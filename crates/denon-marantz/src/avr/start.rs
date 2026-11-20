//! The fast start: the state is usable after one HTTP request, and the rest
//! fills in while the caller already works.

use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::Ordering;
#[cfg(feature = "telnet")]
use std::time::Duration;

#[cfg(feature = "telnet")]
use denon_marantz_proto::telnet::{
    QueryGroup, SignalPart, after_appcommand_queries, signal_queries,
};
use denon_marantz_proto::{
    AUDIO_CHANNELS_PATH, AudioChannelsPage, GroupAvailability, SettingsGroup, State,
    VIDEO_SIGNAL_PATH, VideoSignalPage,
};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

#[cfg(feature = "telnet")]
use super::UpdateMethod;
use super::{Avr, Info};
use crate::details::{PageStatus, SettingsAvailabilityDetails, SkipReason};
use crate::error::Error;
#[cfg(feature = "telnet")]
use crate::telnet::{SyncResult, TelnetStatus};

/// What [`Avr::start_with`] does. The default does everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartOptions {
    signal_pages: bool,
    #[cfg(feature = "heos")]
    heos: bool,
    #[cfg(feature = "telnet")]
    telnet: bool,
    #[cfg(feature = "telnet")]
    telnet_wait: Duration,
}

impl Default for StartOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl StartOptions {
    /// The default: read the signal pages, and open telnet and HEOS.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            signal_pages: true,
            #[cfg(feature = "heos")]
            heos: true,
            #[cfg(feature = "telnet")]
            telnet: true,
            #[cfg(feature = "telnet")]
            telnet_wait: DEFAULT_TELNET_WAIT,
        }
    }

    /// Reads the signal pages of the web interface, which give the video
    /// signal and the channels at once. Without them the receiver gives these
    /// values on telnet, which is slower, or not at all if telnet is not open.
    /// The default is on.
    #[must_use]
    pub const fn signal_pages(mut self, on: bool) -> Self {
        self.signal_pages = on;
        self
    }

    /// Opens the telnet connection, as [`Avr::telnet_connect`] does. The
    /// default is on.
    #[cfg(feature = "telnet")]
    #[must_use]
    pub const fn telnet(mut self, on: bool) -> Self {
        self.telnet = on;
        self
    }

    /// Opens the HEOS connection, as [`Avr::heos_connect`] does, if
    /// [`Info::has_heos`] is true. It runs in the background: neither the
    /// start nor its waits wait for it. See [`Avr::watch_heos_status`] and
    /// [`Avr::heos_start_error`]. The default is on.
    #[cfg(feature = "heos")]
    #[must_use]
    pub const fn heos(mut self, on: bool) -> Self {
        self.heos = on;
        self
    }

    /// How long [`Started::all_synced`] waits for the answers to the telnet
    /// queries. The default is 10 seconds. A start that asks more groups
    /// (see [`TelnetConfig::query_groups`](crate::TelnetConfig::query_groups))
    /// needs more time. Keep it well above
    /// [`TelnetConfig::answer_quiet`](crate::TelnetConfig::answer_quiet):
    /// a query that is not answered ends the wait only after that time.
    #[cfg(feature = "telnet")]
    #[must_use]
    pub const fn telnet_wait(mut self, wait: Duration) -> Self {
        self.telnet_wait = wait;
        self
    }
}

/// The default for [`StartOptions::telnet_wait`].
#[cfg(feature = "telnet")]
const DEFAULT_TELNET_WAIT: Duration = Duration::from_secs(10);

/// What happened to the telnet connection at the start.
#[derive(Debug)]
#[non_exhaustive]
pub enum TelnetStart {
    /// Telnet was not asked for, or the crate is built without it.
    Disabled,
    /// This start opened the connection.
    Open,
    /// A connection was already open or being opened, for example by
    /// `Avr::telnet_connect`. It keeps its own queries.
    AlreadyRunning,
    /// The first connection failed. Nothing keeps trying.
    Failed(Error),
    /// The start was stopped before it finished, so this is not known. It
    /// happens only when the runtime shuts down.
    Unknown,
}

/// What the start did with the HEOS connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HeosStart {
    /// HEOS was not asked for, or the crate is built without the `heos`
    /// feature.
    Disabled,
    /// The receiver has no HEOS, see [`Info::has_heos`].
    NotSupported,
    /// The start opens the connection in the background. See
    /// `Avr::watch_heos_status` for the result and `Avr::heos_start_error`
    /// for the error of a failed connect.
    Started,
    /// A connection was already open or being opened, for example by
    /// `Avr::heos_connect`. It is also reported while `Avr::heos_disconnect`
    /// runs: the start does not open HEOS then.
    AlreadyRunning,
    /// The start was stopped before it finished, so this is not known. It
    /// happens only when the runtime shuts down.
    Unknown,
}

/// The pages of the web interface that the start reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartPages {
    /// The result for each group of settings, as in
    /// [`Avr::update_settings_availability`].
    pub availability: SettingsAvailabilityDetails,
    /// The video signal.
    pub video_signal: PageStatus,
    /// The channels of the audio signal.
    pub audio_channels: PageStatus,
}

/// If the receiver answered the telnet queries of the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TelnetSync {
    /// [`Started::basic_synced`] does not wait for the answers.
    NotWaited,
    /// Each query of the start is answered, or counts as not answered after
    /// [`TelnetConfig::answer_quiet`](crate::TelnetConfig::answer_quiet), see
    /// [`StartReport::telnet_not_answered`].
    Done,
    /// Telnet is not open, so no queries were sent.
    NotOpen,
    /// The connection ended before the receiver answered.
    Lost,
    /// The answers did not end within [`StartOptions::telnet_wait`]. The
    /// queries that were open then are in
    /// [`StartReport::telnet_not_answered`].
    NoAnswer,
}

/// The check of the values from
/// [`AvrBuilder::known`](crate::AvrBuilder::known) at the start, see
/// [`Started::info_check`] and [`Avr::verify`].
#[derive(Debug)]
#[non_exhaustive]
pub enum InfoCheck {
    /// The values were read from the receiver at connect.
    NotNeeded,
    /// The receiver gives the same values.
    Same,
    /// The receiver gives other values. Connect again with them.
    Changed(Box<Info>),
    /// The check failed. [`Error::Stopped`] if the start was stopped before
    /// the check ended.
    Failed(Error),
}

/// What the start did, when it is finished.
#[derive(Debug)]
pub struct StartReport {
    /// The telnet connection.
    pub telnet: TelnetStart,
    /// If the receiver answered the telnet queries. Only [`Started::all_synced`]
    /// waits for this.
    pub telnet_sync: TelnetSync,
    /// The telnet queries that the receiver did not answer, or whose list
    /// did not end, when [`TelnetSync::Done`]. When [`TelnetSync::NoAnswer`],
    /// also the queries that were written without an answer yet and the
    /// queries not written yet. Empty otherwise. Each one is the query text
    /// as sent, without the carriage return, so it can be passed to
    /// [`Avr::telnet_ask`] as [`Ask::Raw`](crate::Ask::Raw).
    pub telnet_not_answered: Vec<Cow<'static, str>>,
    /// What the start did with the HEOS connection. Neither wait waits for
    /// HEOS.
    pub heos: HeosStart,
    /// The signal, the channels, the Audyssey settings and the HDMI output
    /// from `AppCommand0300.xml`, see [`Avr::update_signal_and_settings`].
    /// [`PageStatus::Read`] means that the state has them. Some can be
    /// missing if the receiver left them out, or if the second of the two
    /// requests of the start was not read. [`PageStatus::NotFound`] means
    /// that the receiver does not have the file, and [`PageStatus::NoAnswer`]
    /// that the request timed out or the connection failed.
    pub signal: PageStatus,
    /// The pages of the web interface, or the error if the receiver does not
    /// answer there.
    pub web_interface: Result<StartPages, Error>,
}

/// Marks a start as running, from the call until the task of the start ends.
/// It also ends when the call is dropped or the task is stopped.
struct StartGuard(Arc<Avr>);

impl StartGuard {
    fn new(avr: &Arc<Avr>) -> Result<Self, Error> {
        avr.starting
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| Error::StartRunning)?;
        Ok(Self(Arc::clone(avr)))
    }
}

impl Drop for StartGuard {
    fn drop(&mut self) {
        self.0.starting.store(false, Ordering::Release);
    }
}

/// A start that runs in the background. Dropping it does not stop the start.
///
/// The waits take `&mut self` and keep their result, so they can be called
/// one after the other, and again. A wait that is dropped, for example by a
/// timeout, can be called again.
#[derive(Debug)]
pub struct Started {
    handle: JoinHandle<StartReport>,
    /// Set when the task of the start has ended.
    report: Option<StartReport>,
    /// Gives the check of the known values, until it is in `info_check`.
    info_receiver: Option<oneshot::Receiver<InfoCheck>>,
    info_check: Option<InfoCheck>,
    #[cfg(feature = "telnet")]
    avr: Arc<Avr>,
    #[cfg(feature = "telnet")]
    telnet_wait: Duration,
}

impl Started {
    /// Waits for the check of the values from
    /// [`AvrBuilder::known`](crate::AvrBuilder::known). The start checks them
    /// first, so this is there before the pages are read. Without known
    /// values it is [`InfoCheck::NotNeeded`] at once.
    pub async fn info_check(&mut self) -> &InfoCheck {
        if let Some(receiver) = &mut self.info_receiver {
            let check = receiver
                .await
                .unwrap_or_else(|_| InfoCheck::Failed(Error::Stopped));
            self.info_receiver = None;
            self.info_check = Some(check);
        }
        self.info_check.get_or_insert(InfoCheck::NotNeeded)
    }

    /// Waits until the basic information is in the state: everything that the
    /// start reads over HTTP, the zones and the pages. It does not wait for the
    /// answers to the telnet queries, which can still be on their way:
    /// [`Started::all_synced`] does. The report has [`TelnetSync::NotWaited`]
    /// until `all_synced` ends.
    ///
    /// # Panics
    ///
    /// Panics if the task of the start panicked.
    pub async fn basic_synced(&mut self) -> &StartReport {
        if self.report.is_none() {
            let report = match (&mut self.handle).await {
                Ok(report) => report,
                Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
                Err(_) => stopped_report(),
            };
            self.report = Some(report);
        }
        self.report.get_or_insert_with(stopped_report)
    }

    /// Waits as [`Started::basic_synced`] does, and then until the telnet
    /// queries of the start are answered, as [`Avr::telnet_sync`] does. The
    /// queries that were not answered are in
    /// [`StartReport::telnet_not_answered`]. It takes as long as the paced
    /// queries and the lists do, a few seconds, and ends with
    /// [`TelnetSync::NoAnswer`] after [`StartOptions::telnet_wait`].
    ///
    /// A value that changes later arrives as an event, as always. A caller
    /// that does not need to wait does not have to call this: dropping
    /// [`Started`] does not stop the start.
    ///
    /// # Panics
    ///
    /// Panics if the task of the start panicked.
    pub async fn all_synced(&mut self) -> &StartReport {
        #[cfg_attr(not(feature = "telnet"), expect(unused_variables))]
        let telnet_open = matches!(
            self.basic_synced().await.telnet,
            TelnetStart::Open | TelnetStart::AlreadyRunning
        );
        if self
            .report
            .as_ref()
            .is_some_and(|report| report.telnet_sync == TelnetSync::NotWaited)
        {
            #[cfg(feature = "telnet")]
            let (sync, not_answered) = if telnet_open {
                self.wait_for_telnet().await
            } else {
                (TelnetSync::NotOpen, Vec::new())
            };
            // Without telnet, the start never opens it.
            #[cfg(not(feature = "telnet"))]
            let (sync, not_answered) = (TelnetSync::NotOpen, Vec::new());
            if let Some(report) = &mut self.report {
                report.telnet_sync = sync;
                report.telnet_not_answered = not_answered;
            }
        }
        self.report.get_or_insert_with(stopped_report)
    }

    #[cfg(feature = "telnet")]
    async fn wait_for_telnet(&self) -> (TelnetSync, Vec<Cow<'static, str>>) {
        match self.avr.telnet_sync_start(self.telnet_wait).await {
            Ok(SyncResult::Done(not_answered)) => (TelnetSync::Done, not_answered),
            Ok(SyncResult::TimedOut(open)) => (TelnetSync::NoAnswer, open),
            Err(_) => (TelnetSync::Lost, Vec::new()),
        }
    }
}

/// The report of a start that was stopped before it ended. It happens only
/// when the runtime shuts down.
const fn stopped_report() -> StartReport {
    StartReport {
        telnet: TelnetStart::Unknown,
        telnet_sync: TelnetSync::NotWaited,
        telnet_not_answered: Vec::new(),
        heos: HeosStart::Unknown,
        signal: PageStatus::NoAnswer,
        web_interface: Err(Error::Stopped),
    }
}

impl Avr {
    /// Starts the receiver with the default [`StartOptions`], see
    /// [`Avr::start_with`].
    ///
    /// # Errors
    ///
    /// The errors of [`Avr::update`].
    pub async fn start(self: &Arc<Self>) -> Result<Started, Error> {
        self.start_with(StartOptions::new()).await
    }

    /// Fills the state as fast as the receiver allows.
    ///
    /// It reads the zones with [`Avr::update`]. If that works, it opens telnet
    /// and returns, so the state is usable then. If the telnet port does not
    /// answer, the call waits for the connect timeout of the telnet settings
    /// first. In the background it asks telnet first for what the HTTP read
    /// does not give. It also reads, one after the other: the video signal
    /// page, the audio signal and the channels with `AppCommand0300.xml`, the
    /// rest of [`Avr::update_signal_and_settings`], the pages that say which
    /// settings are available, and the channels page if `AppCommand0300.xml`
    /// did not give the channels. A page that fails is tried once more. The
    /// telnet queries for the signal that the pages answer are not sent,
    /// unless a page could not be read. A page that does not answer after
    /// another page did is reported as [`PageStatus::NoAnswer`]. If the web
    /// interface refuses the connection for the video page, or neither the
    /// video page nor the first page after it answers, the other pages are
    /// not asked. A video page that could not connect for another reason is
    /// read again after the other pages.
    ///
    /// Use [`Started::basic_synced`] to wait for the HTTP reads and
    /// [`Started::all_synced`] to wait for the telnet answers too. The values
    /// from telnet arrive as events later. Calls that follow, for example commands,
    /// need not wait.
    ///
    /// # Errors
    ///
    /// The errors of [`Avr::update`]. Telnet is not opened then. A failure of
    /// telnet or of the web interface is in the report.
    ///
    /// [`Error::StartRunning`] if another start of this receiver is still
    /// running, also in the background. Nothing is read then. A start after
    /// a finished one reads again.
    ///
    /// If the future is dropped before it returns, for example by a timeout,
    /// no connection stays open.
    pub async fn start_with(self: &Arc<Self>, options: StartOptions) -> Result<Started, Error> {
        let running = StartGuard::new(self)?;
        self.update().await?;
        #[cfg(feature = "telnet")]
        let telnet = if options.telnet {
            self.start_telnet().await
        } else {
            TelnetStart::Disabled
        };
        #[cfg(not(feature = "telnet"))]
        let telnet = TelnetStart::Disabled;
        #[cfg(feature = "heos")]
        let heos = self.start_heos(options);
        #[cfg(not(feature = "heos"))]
        let heos = HeosStart::Disabled;
        let avr = Arc::clone(self);
        let task = Arc::clone(&avr);
        #[cfg(feature = "telnet")]
        let telnet_wait = options.telnet_wait;
        let (info_sender, info_receiver, info_check) = if self.known {
            let (sender, receiver) = oneshot::channel();
            (Some(sender), Some(receiver), None)
        } else {
            (None, None, Some(InfoCheck::NotNeeded))
        };
        Ok(Started {
            handle: tokio::spawn(async move {
                let report = task.finish_reads(options, telnet, info_sender).await;
                let report = StartReport { heos, ..report };
                drop(running);
                report
            }),
            report: None,
            info_receiver,
            info_check,
            #[cfg(feature = "telnet")]
            avr,
            #[cfg(feature = "telnet")]
            telnet_wait,
        })
    }

    /// Opens telnet. After an `AppCommand` read, the first queries are the
    /// ones that it does not answer, and the groups of the settings that are
    /// not in [`QueryGroup::START_ORDER`](crate::QueryGroup::START_ORDER)
    /// follow. Another receiver gets the groups of the settings. The queries for the signal give the first value. A receiver
    /// with `AppCommand0300.xml` gives the kind of the audio and the sampling
    /// rate there, and is asked only if it does not answer. Later changes come
    /// as events if the receiver sends them, see [`Avr::refresh_signal`].
    #[cfg(feature = "telnet")]
    async fn start_telnet(&self) -> TelnetStart {
        let first = (self.info.update_method == UpdateMethod::AppCommand).then(|| {
            let brand = self.info.brand;
            // `AppCommand0300.xml` gives the audio values, or the fallback
            // asks for them.
            let mut queries: Vec<&'static str> = after_appcommand_queries(brand)
                .filter(|query| self.telnet.wanted(query))
                .collect();
            queries.extend(signal_queries(brand, SignalPart::TelnetOnly));
            // The groups that were asked for, after the ones that are needed first.
            for query in self.telnet.extra_start_queries() {
                if !queries.contains(&query) {
                    queries.push(query);
                }
            }
            queries
        });
        match self
            .telnet
            .connect(&self.info.host, &self.channels, first)
            .await
        {
            Ok(true) => TelnetStart::Open,
            Ok(false) => TelnetStart::AlreadyRunning,
            Err(error) => TelnetStart::Failed(error),
        }
    }

    /// Opens HEOS in the background, without waiting for it.
    #[cfg(feature = "heos")]
    fn start_heos(&self, options: StartOptions) -> HeosStart {
        if !options.heos {
            HeosStart::Disabled
        } else if !self.info.has_heos() {
            HeosStart::NotSupported
        } else if self.heos.connect_in_background() {
            HeosStart::Started
        } else {
            HeosStart::AlreadyRunning
        }
    }

    /// The reads of the start. The report has [`HeosStart::Disabled`].
    /// `info_sender` is set for known values, which are checked first, so
    /// that [`Started::info_check`] has the result soon.
    async fn finish_reads(
        &self,
        options: StartOptions,
        telnet: TelnetStart,
        info_sender: Option<oneshot::Sender<InfoCheck>>,
    ) -> StartReport {
        if let Some(sender) = info_sender {
            let check = match self.verify().await {
                Ok(None) => InfoCheck::Same,
                Ok(Some(info)) => InfoCheck::Changed(Box::new(info)),
                Err(error) => InfoCheck::Failed(error),
            };
            // Fails only if the caller dropped `Started`.
            let _ = sender.send(check);
        }
        // The video page first: it gives HDR, the color space and the pixel
        // depth, which clients need soon.
        let video = if options.signal_pages {
            Some(self.read_video_page().await)
        } else {
            None
        };
        // Then the audio signal and the channels, and the rest after them.
        let signal = self
            .update_signal_and_settings_in_parts()
            .await
            .unwrap_or(PageStatus::NoAnswer);
        let after_video = VideoPage::of(video.as_ref());
        let web_interface = match video {
            Some(Err(error)) if after_video == VideoPage::Refused => Err(error),
            video => {
                let video_signal = video.map(|result| result.unwrap_or(PageStatus::NoAnswer));
                self.read_start_pages(video_signal, after_video).await
            }
        };
        #[cfg(feature = "telnet")]
        if matches!(telnet, TelnetStart::Open)
            && self.info.update_method == UpdateMethod::AppCommand
        {
            self.ask_for_signal(&web_interface).await;
        }
        StartReport {
            telnet,
            telnet_sync: TelnetSync::NotWaited,
            telnet_not_answered: Vec::new(),
            heos: HeosStart::Disabled,
            signal,
            web_interface,
        }
    }

    /// Asks on telnet for the signal values that the pages did not give, and
    /// for the colorimetry last.
    #[cfg(feature = "telnet")]
    async fn ask_for_signal(&self, web_interface: &Result<StartPages, Error>) {
        let brand = self.info.brand;
        let (video, mut channels) = web_interface.as_ref().map_or((false, false), |pages| {
            (
                pages.video_signal.has_values(),
                pages.audio_channels.has_values(),
            )
        });
        // The answer of `AppCommand0300.xml` has the channels, also when the
        // pages are not read.
        channels |= self.signal_from_appcommand.load(Ordering::Relaxed);
        let mut queries = Vec::new();
        // The first queries left these out for a receiver with `AppCommand0300.xml`.
        if self.has_appcommand0300() && !self.signal_from_appcommand.load(Ordering::Relaxed) {
            queries.extend(signal_queries(brand, SignalPart::AudioInfo));
        }
        if !channels {
            queries.extend(signal_queries(brand, SignalPart::Channels));
        }
        if !video {
            queries.extend(signal_queries(brand, SignalPart::Video));
        }
        // Last, after the pages and the video queries, so that the answer is
        // for the signal that the state has.
        queries.extend(signal_queries(brand, SignalPart::Colorimetry));
        if self.telnet.ask(queries).await.is_err() {
            tracing::debug!(host = %self.info.host, "signal queries not sent, telnet closed");
        }
    }

    /// The receiver is read with `AppCommand`, so `AppCommand0300.xml` is
    /// asked. The first queries leave out the audio values for it, and
    /// `update_signal_and_settings` or the fallback after it gives them.
    #[cfg(feature = "telnet")]
    fn has_appcommand0300(&self) -> bool {
        self.info.update_method == UpdateMethod::AppCommand
    }

    async fn read_video_page(&self) -> Result<PageStatus, Error> {
        let result = self
            .read_web_interface(VIDEO_SIGNAL_PATH, VideoSignalPage::parse, |state, page| {
                state.apply_video_signal_page(&page)
            })
            .await;
        self.forget_telnet_values(&result);
        self.ask_colorimetry_if_unknown().await;
        result
    }

    /// Asks telnet for the colorimetry if a change made BT.2020 not known, see
    /// [`SignalState::needs_colorimetry_query`](denon_marantz_proto::SignalState::needs_colorimetry_query).
    #[cfg_attr(
        not(feature = "telnet"),
        allow(clippy::unused_async, clippy::unused_self)
    )]
    pub(super) async fn ask_colorimetry_if_unknown(&self) {
        #[cfg(feature = "telnet")]
        if self
            .channels
            .read(|state| state.device().signal.needs_colorimetry_query())
            && self
                .telnet
                .ask(signal_queries(self.info.brand, SignalPart::Colorimetry))
                .await
                .is_err()
        {
            tracing::debug!(host = %self.info.host, "colorimetry query not sent, telnet closed");
        }
    }

    async fn read_channels_page(&self) -> Result<PageStatus, Error> {
        let result = self
            .read_web_interface(
                AUDIO_CHANNELS_PATH,
                AudioChannelsPage::parse,
                |state, page| state.apply_audio_channels_page(&page),
            )
            .await;
        self.forget_telnet_values(&result);
        result
    }

    /// After a page set values, the next telnet event with the old value is a
    /// real change, so telnet must not drop it as a repeat.
    // Without telnet there is nothing to forget, and the body is empty.
    #[cfg_attr(
        not(feature = "telnet"),
        allow(clippy::missing_const_for_fn, clippy::unused_self)
    )]
    pub(super) fn forget_telnet_values(&self, result: &Result<PageStatus, Error>) {
        #[cfg(feature = "telnet")]
        if matches!(result, Ok(PageStatus::Read)) {
            self.telnet.forget_values();
        }
        #[cfg(not(feature = "telnet"))]
        let _ = result;
    }

    /// Reads the video and audio signal again, in the way that fits.
    ///
    /// If telnet is open, only telnet is asked, and the values arrive as
    /// events after the call returns. Else `AppCommand0300.xml`, if the
    /// receiver has it, and the web interface are read: the video signal and
    /// the channels. Your code does not choose. Auto low latency mode and
    /// quick frame transport come only from the web interface.
    ///
    /// The signal does not send changes over HTTP. Some receivers do not send
    /// video signal changes on telnet. Telnet asks for them after a hint, see
    /// [`TelnetConfig::signal_wait_min`](crate::TelnetConfig::signal_wait_min).
    /// Call this to see the signal now, or on a timer if the receiver sends
    /// neither changes nor hints. To read the pages also while telnet is
    /// open, call [`Avr::refresh_signal_pages`].
    ///
    /// # Errors
    ///
    /// Returns an error if nothing could be asked or read: telnet is not open
    /// and the web interface does not answer, or answers with no page that
    /// can be read ([`Error::NothingRead`]).
    pub async fn refresh_signal(&self) -> Result<(), Error> {
        #[cfg(feature = "telnet")]
        if *self.telnet.watch_status().borrow() == TelnetStatus::Connected {
            // The group asks BT.2020 last, so that its answer comes after the
            // video values.
            match self.telnet.refresh_group(QueryGroup::Signal).await {
                // Telnet closed after the check: read the web interface.
                Err(Error::NotConnected) => {}
                result => return result,
            }
        }
        self.refresh_signal_pages().await
    }

    /// Reads the video and audio signal over HTTP, also while telnet is open:
    /// `AppCommand0300.xml`, if the receiver has it, the video signal page
    /// and, without an answer of `AppCommand0300.xml`, the channels page.
    /// While telnet is open, it asks telnet only for BT.2020 when the page
    /// changed the signal and the receiver reports BT.2020 on telnet.
    ///
    /// Use it to read pages that failed before, for example at the start.
    /// Auto low latency mode and quick frame transport come only from the
    /// video signal page. For the signal now, call [`Avr::refresh_signal`].
    ///
    /// # Errors
    ///
    /// Returns an error if the web interface does not answer, or answers with
    /// no page that can be read ([`Error::NothingRead`]).
    pub async fn refresh_signal_pages(&self) -> Result<(), Error> {
        // A receiver that answers `AppCommand0300.xml` gives the resolution, the
        // audio signal and the channels at once, and its answer for the
        // channels is more complete than the page.
        let answered = matches!(
            self.update_signal_and_settings().await,
            Ok(PageStatus::Read)
        );
        let video = self.read_video_page().await;
        let channels = if answered {
            Ok(PageStatus::Skipped(SkipReason::AlreadyRead))
        } else {
            self.read_channels_page().await
        };
        let is_read =
            |result: &Result<PageStatus, Error>| result.as_ref().is_ok_and(|s| s.has_values());
        if answered || is_read(&video) || is_read(&channels) {
            return Ok(());
        }
        // Nothing was read: say why.
        video?;
        channels?;
        Err(Error::NothingRead)
    }

    /// Reads the pages of the start after the video page. `video_signal` is
    /// the status of the video page, or `None` if the signal pages are not
    /// asked.
    async fn read_start_pages(
        &self,
        video_signal: Option<PageStatus>,
        after_video: VideoPage,
    ) -> Result<StartPages, Error> {
        let mut pages = Vec::with_capacity(SettingsGroup::ALL.len());
        for group in SettingsGroup::ALL {
            let result = self
                .read_web_interface(
                    group.path(),
                    |body| GroupAvailability::parse(group, body),
                    State::apply_settings_availability,
                )
                .await;
            // The first answer, also the one of the video page, shows that the
            // web interface is there. A page that does not answer after that
            // is a problem of the page: the receiver is busy, and the other
            // pages are still read.
            let status = if pages.is_empty() && after_video != VideoPage::Answered {
                result?
            } else {
                result.unwrap_or(PageStatus::NoAnswer)
            };
            pages.push((group, status));
        }
        let (video_signal, audio_channels) = if let Some(video_signal) = video_signal {
            let channels = if self.signal_from_appcommand.load(Ordering::Relaxed) {
                // The answer of `AppCommand0300.xml` has the channels. The page
                // has none for a stream of objects, and would clear them.
                PageStatus::Skipped(SkipReason::AlreadyRead)
            } else {
                self.read_channels_page()
                    .await
                    .unwrap_or(PageStatus::NoAnswer)
            };
            let video_signal = if after_video == VideoPage::NotSent {
                let result = self.read_video_page().await;
                tracing::debug!(host = %self.info.host, ?result, "video signal page read again");
                result.unwrap_or(PageStatus::NoAnswer)
            } else {
                video_signal
            };
            (video_signal, channels)
        } else {
            (
                PageStatus::Skipped(SkipReason::NotAsked),
                PageStatus::Skipped(SkipReason::NotAsked),
            )
        };
        Ok(StartPages {
            availability: SettingsAvailabilityDetails { pages },
            video_signal,
            audio_channels,
        })
    }
}

/// What the start does after the video page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VideoPage {
    /// The signal pages are not asked. The first page that is read decides
    /// if the web interface is there.
    NotAsked,
    /// The web interface answered, so the other pages are all read.
    Answered,
    /// Nothing listens on the port, so the other pages are not asked.
    Refused,
    /// The request was sent but had no answer. The first page after it
    /// decides if the web interface is there.
    NoAnswer,
    /// The request was not sent: the network of the caller can be down for
    /// a moment, for example after a wake-up. The first page after it
    /// decides, and the video page is read again after the other pages.
    NotSent,
}

impl VideoPage {
    fn of(video: Option<&Result<PageStatus, Error>>) -> Self {
        match video {
            None => Self::NotAsked,
            Some(Ok(_)) => Self::Answered,
            Some(Err(error)) if error.is_connection_refused() => Self::Refused,
            Some(Err(error)) if error.is_not_sent() => Self::NotSent,
            Some(Err(_)) => Self::NoAnswer,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;
    use std::time::Duration;

    use super::*;

    fn network(kind: ErrorKind, sent: bool) -> Error {
        Error::Network {
            request: "GET /".to_owned(),
            source: Box::new(std::io::Error::from(kind)),
            sent,
        }
    }

    fn timeout(sent: bool) -> Error {
        Error::Timeout {
            request: "GET /".to_owned(),
            limit: Duration::from_secs(1),
            sent,
        }
    }

    #[test]
    fn the_result_of_the_video_page_decides_what_comes_after_it() {
        let of = |result: Result<PageStatus, Error>| VideoPage::of(Some(&result));
        assert_eq!(VideoPage::of(None), VideoPage::NotAsked);
        assert_eq!(of(Ok(PageStatus::Read)), VideoPage::Answered);
        assert_eq!(of(Ok(PageStatus::NotFound)), VideoPage::Answered);
        assert_eq!(
            of(Err(network(ErrorKind::ConnectionRefused, false))),
            VideoPage::Refused
        );
        // A wake-up of the caller.
        assert_eq!(
            of(Err(network(ErrorKind::NetworkUnreachable, false))),
            VideoPage::NotSent
        );
        // A port that drops the connection attempt.
        assert_eq!(of(Err(timeout(false))), VideoPage::NotSent);
        assert_eq!(of(Err(timeout(true))), VideoPage::NoAnswer);
        assert_eq!(
            of(Err(network(ErrorKind::ConnectionReset, true))),
            VideoPage::NoAnswer
        );
    }
}
