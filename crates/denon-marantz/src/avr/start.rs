//! The fast start: the state is usable after one HTTP request, and the rest
//! fills in while the caller already works.

use std::sync::Arc;

#[cfg(feature = "telnet")]
use denon_marantz_proto::telnet::{SignalPart, after_appcommand_queries, signal_queries};
use denon_marantz_proto::{
    AUDIO_CHANNELS_PATH, AudioChannelsPage, GroupAvailability, SettingsGroup, State,
    VIDEO_SIGNAL_PATH, VideoSignalPage,
};
use tokio::task::JoinHandle;

use super::Avr;
#[cfg(feature = "telnet")]
use super::UpdateMethod;
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
}

/// What happened to the telnet connection at the start.
#[derive(Debug)]
#[non_exhaustive]
pub enum TelnetStart {
    /// Telnet was not asked for, or the crate is built without it.
    Disabled,
    /// The connection is open.
    Open,
    /// The first connection failed. Nothing keeps trying.
    Failed(Error),
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

/// What the start did, when it is finished.
#[derive(Debug)]
pub struct StartReport {
    /// The telnet connection.
    pub telnet: TelnetStart,
    /// The pages of the web interface, or the error if the receiver does not
    /// answer there.
    pub web_interface: Result<StartPages, Error>,
}

/// A start that runs in the background. Dropping it does not stop the start.
#[derive(Debug)]
pub struct Started {
    handle: JoinHandle<StartReport>,
}

impl Started {
    /// Waits until everything that the start reads has been read, and says
    /// what happened. The values are in the state before this returns, and
    /// the answers of telnet queries can still be on their way.
    ///
    /// # Panics
    ///
    /// Panics if the task of the start panicked.
    pub async fn finished(self) -> StartReport {
        match self.handle.await {
            Ok(report) => report,
            Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
            Err(_) => StartReport {
                telnet: TelnetStart::Disabled,
                web_interface: Err(Error::Stopped),
            },
        }
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
    /// It reads the zones with [`Avr::update`] and returns when that is done,
    /// so the state is usable then. In the background it opens telnet at the
    /// same time, and asks it first for what the HTTP read does not give. It
    /// also reads, one after the other, the pages that say which settings are
    /// available, and the signal pages, with one more try for a page that
    /// fails. The telnet queries for the signal that the pages answer are not
    /// sent, unless a page could not be read.
    ///
    /// Use [`Started::finished`] to wait for the rest. The values from
    /// telnet arrive as events later. Calls that follow, for example commands,
    /// need not wait.
    ///
    /// # Errors
    ///
    /// The errors of [`Avr::update`]. The telnet connection is closed again
    /// then. A failure of telnet or of the web interface is in the report.
    pub async fn start_with(self: &Arc<Self>, options: StartOptions) -> Result<Started, Error> {
        #[cfg(feature = "telnet")]
        let (read, telnet) = if options.telnet {
            tokio::join!(self.update(), self.start_telnet())
        } else {
            (self.update().await, TelnetStart::Disabled)
        };
        #[cfg(not(feature = "telnet"))]
        let (read, telnet) = (self.update().await, TelnetStart::Disabled);
        #[cfg(feature = "telnet")]
        if read.is_err() {
            self.telnet_disconnect().await;
        }
        read?;
        let avr = Arc::clone(self);
        Ok(Started {
            handle: tokio::spawn(async move { avr.finish_start(options, telnet).await }),
        })
    }

    /// Opens telnet. After an `AppCommand` read, the first queries are the
    /// ones that it does not answer. Another receiver gets the groups of the
    /// settings.
    #[cfg(feature = "telnet")]
    async fn start_telnet(&self) -> TelnetStart {
        let first = (self.info.update_method == UpdateMethod::AppCommand).then(|| {
            after_appcommand_queries(self.info.brand)
                .chain(signal_queries(self.info.brand, SignalPart::Audio))
                .collect()
        });
        match self
            .telnet
            .connect(&self.info.host, &self.channels, first)
            .await
        {
            Ok(()) => TelnetStart::Open,
            Err(error) => TelnetStart::Failed(error),
        }
    }

    async fn finish_start(&self, options: StartOptions, telnet: TelnetStart) -> StartReport {
        let web_interface = self.read_start_pages(options).await;
        #[cfg(feature = "telnet")]
        if matches!(telnet, TelnetStart::Open)
            && self.info.update_method == UpdateMethod::AppCommand
        {
            self.ask_for_signal(&web_interface).await;
        }
        StartReport {
            telnet,
            web_interface,
        }
    }

    /// Asks on telnet for the signal values that the pages did not give.
    #[cfg(feature = "telnet")]
    async fn ask_for_signal(&self, web_interface: &Result<StartPages, Error>) {
        let brand = self.info.brand;
        let (video, channels) = web_interface.as_ref().map_or((false, false), |pages| {
            (pages.video_signal.is_read(), pages.audio_channels.is_read())
        });
        let mut queries = Vec::new();
        if !video {
            queries.extend(signal_queries(brand, SignalPart::Video));
        }
        if !channels {
            queries.extend(signal_queries(brand, SignalPart::Channels));
        }
        if !queries.is_empty() && self.telnet.ask(queries).await.is_err() {
            tracing::debug!(host = %self.info.host, "signal queries not sent, telnet closed");
        }
    }

    /// Reads the video page and the channel page of the web interface.
    async fn read_signal_pages(&self) -> Result<(PageStatus, PageStatus), Error> {
        let video = self
            .read_web_interface(VIDEO_SIGNAL_PATH, VideoSignalPage::parse, |state, page| {
                state.apply_video_signal_page(&page)
            })
            .await?;
        let channels = self
            .read_web_interface(
                AUDIO_CHANNELS_PATH,
                AudioChannelsPage::parse,
                |state, page| state.apply_audio_channels_page(&page),
            )
            .await?;
        Ok((video, channels))
    }

    /// Reads the video and audio signal again, in the way that fits.
    ///
    /// The web interface answers the video signal and the channels at once.
    /// If telnet is open, it is asked for the rest: the audio signal, the
    /// sampling rate and the HDMI limit, and for the video signal and the
    /// channels too if their page could not be read. Your code does not
    /// choose. The values from telnet arrive as events after the call returns.
    ///
    /// The signal does not send changes over HTTP, and telnet reports it only
    /// when it is asked, so call this to see the signal now, for example when
    /// the source changed.
    ///
    /// # Errors
    ///
    /// Returns an error if nothing could be asked: the web interface does
    /// not answer and telnet is not open.
    pub async fn refresh_signal(&self) -> Result<(), Error> {
        let pages = self.read_signal_pages().await;
        #[cfg(feature = "telnet")]
        if *self.telnet.watch_status().borrow() == TelnetStatus::Connected {
            let (video, channels) = pages.as_ref().map_or((false, false), |(video, channels)| {
                (video.is_read(), channels.is_read())
            });
            let mut queries: Vec<&'static str> =
                signal_queries(self.info.brand, SignalPart::Audio).collect();
            if !video {
                queries.extend(signal_queries(self.info.brand, SignalPart::Video));
            }
            if !channels {
                queries.extend(signal_queries(self.info.brand, SignalPart::Channels));
            }
            return self.telnet.ask(queries).await;
        }
        pages.map(|_| ())
    }

    async fn read_start_pages(&self, options: StartOptions) -> Result<StartPages, Error> {
        let mut pages = Vec::with_capacity(SettingsGroup::ALL.len());
        for group in SettingsGroup::ALL {
            let status = self
                .read_web_interface(
                    group.path(),
                    |body| GroupAvailability::parse(group, body),
                    State::apply_settings_availability,
                )
                .await?;
            pages.push((group, status));
        }
        let (video_signal, audio_channels) = if options.signal_pages {
            self.read_signal_pages().await?
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
