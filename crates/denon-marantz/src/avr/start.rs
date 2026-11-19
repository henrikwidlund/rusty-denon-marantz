//! The fast start: the state is usable after one HTTP request, and the rest
//! fills in while the caller already works.

use std::sync::Arc;
use std::sync::atomic::Ordering;
#[cfg(feature = "telnet")]
use std::time::Duration;

#[cfg(feature = "telnet")]
use denon_marantz_proto::telnet::{SignalPart, after_appcommand_queries, signal_queries};
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
use crate::telnet::TelnetStatus;

/// What [`Avr::start_with`] does. The default does everything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartOptions {
    signal_pages: bool,
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
    /// The default: read the signal pages, and open telnet.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            signal_pages: true,
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

    /// How long [`Started::all_synced`] waits for the answers to the telnet
    /// queries. The default is 10 seconds. A start that asks more groups
    /// (see [`TelnetConfig::query_groups`](crate::TelnetConfig::query_groups))
    /// needs more time.
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
    /// The receiver answered a last query that was sent after the queries of
    /// the start. It handles lines in order, so the state has the values that
    /// it gave to the queries before it.
    Answered,
    /// Telnet is not open, so no queries were sent.
    NotOpen,
    /// The connection ended before the receiver answered.
    Lost,
    /// The receiver did not answer the last query in time, see
    /// [`StartOptions::telnet_wait`].
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
    /// The signal, the channels, the Audyssey settings and the HDMI output
    /// from `AppCommand0300.xml`, see [`Avr::update_signal_and_settings`].
    /// [`PageStatus::Read`] means that the state has them. Some can be
    /// missing: telnet is then asked for them. [`PageStatus::NotFound`]
    /// means that the receiver does not have the file, and
    /// [`PageStatus::NoAnswer`] that the request timed out or the connection
    /// failed.
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

    /// Waits as [`Started::basic_synced`] does, and then until the receiver has
    /// answered a last query that is sent after the telnet queries of the
    /// start. The receiver handles lines in order, so the answers to the
    /// earlier queries are in the state then. A value that is still empty is
    /// probably one that this receiver does not answer. The library cannot
    /// check this: an event that looks like the answer, or a query that the
    /// receiver drops, can leave a value for later. It takes as long as the
    /// paced queries do, a few seconds, and ends with
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
            let sync = if telnet_open {
                self.wait_for_telnet().await
            } else {
                TelnetSync::NotOpen
            };
            // Without telnet, the start never opens it.
            #[cfg(not(feature = "telnet"))]
            let sync = TelnetSync::NotOpen;
            if let Some(report) = &mut self.report {
                report.telnet_sync = sync;
            }
        }
        self.report.get_or_insert_with(stopped_report)
    }

    #[cfg(feature = "telnet")]
    async fn wait_for_telnet(&self) -> TelnetSync {
        match self.avr.telnet_sync_within(self.telnet_wait).await {
            Ok(()) => TelnetSync::Answered,
            Err(error) if error.is_timeout() => TelnetSync::NoAnswer,
            Err(_) => TelnetSync::Lost,
        }
    }
}

/// The report of a start that was stopped before it ended. It happens only
/// when the runtime shuts down.
const fn stopped_report() -> StartReport {
    StartReport {
        telnet: TelnetStart::Unknown,
        telnet_sync: TelnetSync::NotWaited,
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
                let report = task.finish_start(options, telnet, info_sender).await;
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
            let mut queries: Vec<&'static str> = after_appcommand_queries(brand).collect();
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

    /// `info_sender` is set for known values, which are checked first, so
    /// that [`Started::info_check`] has the result soon.
    async fn finish_start(
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
        let web_interface = match video {
            // Nothing listens on the port, so the other pages are not asked.
            Some(Err(error)) if error.is_connection_refused() => Err(error),
            video => self.read_start_pages(video).await,
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
    /// The web interface answers the video signal and the channels at once.
    /// If telnet is open, it is asked for the rest: the audio signal, the
    /// sampling rate and the HDMI limit, and for the video signal and the
    /// channels too if their page could not be read. Your code does not
    /// choose. The values from telnet arrive as events after the call returns.
    ///
    /// The signal does not send changes over HTTP. Some receivers send changes
    /// on telnet by themselves, and others do not. Call this to see the signal
    /// now, for example when the source changed, or on a timer if the receiver
    /// does not send changes. The library does not call it by itself.
    ///
    /// # Errors
    ///
    /// Returns an error if nothing could be asked or read: telnet is not open
    /// and the web interface does not answer, or answers with no page that
    /// can be read ([`Error::NothingRead`]).
    pub async fn refresh_signal(&self) -> Result<(), Error> {
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
        #[cfg(feature = "telnet")]
        if *self.telnet.watch_status().borrow() == TelnetStatus::Connected {
            let (video, channels) = (is_read(&video), is_read(&channels));
            let mut queries: Vec<&'static str> =
                signal_queries(self.info.brand, SignalPart::TelnetOnly).collect();
            // `AppCommand0300.xml` gave the audio values, if it answered.
            if !answered {
                queries.extend(signal_queries(self.info.brand, SignalPart::AudioInfo));
            }
            if !video {
                queries.extend(signal_queries(self.info.brand, SignalPart::Video));
            }
            if !channels {
                queries.extend(signal_queries(self.info.brand, SignalPart::Channels));
            }
            // Last, so that its answer comes after the video values.
            queries.extend(signal_queries(self.info.brand, SignalPart::Colorimetry));
            return self.telnet.ask(queries).await;
        }
        if answered || is_read(&video) || is_read(&channels) {
            return Ok(());
        }
        // Nothing was read: say why.
        video?;
        channels?;
        Err(Error::NothingRead)
    }

    /// Reads the pages of the start after the video page. `video` is the
    /// result of the video page, or `None` if the signal pages are not asked.
    async fn read_start_pages(
        &self,
        video: Option<Result<PageStatus, Error>>,
    ) -> Result<StartPages, Error> {
        let answered = matches!(video, Some(Ok(_)));
        // The network of the caller can be down for a moment, for example
        // after a wake-up. The page is read again after the others then.
        let read_video_again = matches!(&video, Some(Err(error)) if error.is_not_sent());
        let video_signal = video.map(|result| result.unwrap_or(PageStatus::NoAnswer));
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
            let status = if pages.is_empty() && !answered {
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
            let video_signal = if read_video_again {
                self.read_video_page().await.unwrap_or(PageStatus::NoAnswer)
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
