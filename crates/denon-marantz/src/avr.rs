//! A receiver: identification, state and updates over HTTP.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bytes::Bytes;
use denon_marantz_proto::appcommand::{
    self, Cmd, Definition, Endpoint, Field, Response, write_body,
};
use denon_marantz_proto::status_page::{self, Page};
use denon_marantz_proto::telnet::Command;
#[cfg(feature = "telnet")]
use denon_marantz_proto::telnet::QueryGroup;
use denon_marantz_proto::{
    BASS_PATH, BassSettings, Brand, CHANNEL_LEVELS_PATH, CROSSOVERS_PATH, Change, DISTANCES_PATH,
    DeviceDescription, GeneralPage, GroupAvailability, HD_RADIO_STATUS_PATH, INPUT_ASSIGN_PATH,
    InputAssignment, NET_AUDIO_STATUS_PATH, NowPlaying, PLAYBACK_2CH_PATH, Playback2Ch,
    PlayingKind, ReceiverType, SOURCE_LEVELS_PATH, SOURCE_NAMES_PATH, SYSTEM_SETTINGS_PATH,
    SettingsGroup, SourceLevels, SourceNames, SpeakerTable, State, SystemSettings,
    TUNER_STATUS_PATH, TunerPage, VideoMenuPage, WEB_INTERFACE_PORT, XLR_ASSIGN_PATH, XlrAssign,
    Zone,
};
use tokio::sync::{Mutex, broadcast, watch};

use crate::audyssey::Audyssey;
use crate::channels::Channels;
use crate::command::{self, Reads};
use crate::details::{
    GeneralDetails, PageResults, PageStatus, SettingsAvailabilityDetails, SkipReason,
    SourceDetails, SpeakerDetails, VideoMenuDetails,
};
use crate::device_control::{DeviceControl, ResetKeys};
use crate::dirac::Dirac;
use crate::error::Error;
use crate::http::{HttpClient, HttpConfig};
use crate::identify::{Identified, identify, identify_on};
use crate::pacer::Pacing;
use crate::sound_control::SoundControl;
#[cfg(feature = "telnet")]
use crate::telnet::{Ask, Telnet, TelnetConfig, TelnetEvent, TelnetStatus};
use crate::tone_control::ToneControl;
use crate::tuner_control::TunerControl;
use crate::video_control::VideoControl;
use crate::zone_control::ZoneControl;

mod follow_up;
mod start;

use follow_up::FollowUp;

pub use follow_up::FollowUpRead;
pub use start::{StartOptions, StartPages, StartReport, Started, TelnetStart, TelnetSync};

/// Name of a receiver that has none.
const DEFAULT_NAME: &str = "AVR";

/// Queries that fill the state. The answer has one item for each, in this order.
/// Receivers of the 2016 generation and later get [`appcommand::GET_TONE_CONTROL`]
/// after them.
const UPDATE_COMMANDS: [Definition; 12] = [
    appcommand::GET_ALL_ZONE_POWER_STATUS,
    appcommand::GET_AUTO_STANDBY,
    appcommand::GET_DIMMER,
    appcommand::GET_ECO,
    appcommand::GET_ALL_ZONE_SOURCE,
    appcommand::GET_ALL_ZONE_VOLUME,
    appcommand::GET_ALL_ZONE_MUTE_STATUS,
    appcommand::GET_SURROUND_MODE_STATUS,
    appcommand::GET_RENAME_SOURCE,
    appcommand::GET_DELETED_SOURCE,
    appcommand::GET_ZONE_NAME,
    appcommand::GET_FRIENDLY_NAME,
];

/// The queries of `AppCommand0300.xml` that [`Avr::update_signal`] sends for a
/// receiver of the 2016 generation and later: the Audyssey settings, the
/// video and audio signal, and the channels of the signal and of the speakers.
const APPCOMMAND0300_COMMANDS: [Definition; 5] = [
    appcommand::GET_AUDYSSEY,
    appcommand::GET_VIDEO_INFO,
    appcommand::GET_AUDIO_INFO,
    appcommand::GET_INPUT_SIGNAL,
    appcommand::GET_ACTIVE_SPEAKER,
];

/// The commands that read what telnet does not report. [`Avr::refresh`] sends
/// only these when the telnet connection is open.
#[cfg(feature = "telnet")]
const HTTP_ONLY_COMMANDS: [Definition; 1] = [appcommand::GET_ZONE_NAME];

/// [`HTTP_ONLY_COMMANDS`] and the settings of [`QueryGroup::Device`]. The
/// refresh sends these when the telnet connection does not fetch that group.
#[cfg(feature = "telnet")]
const HTTP_ONLY_WITH_DEVICE_COMMANDS: [Definition; 4] = [
    appcommand::GET_ZONE_NAME,
    appcommand::GET_AUTO_STANDBY,
    appcommand::GET_DIMMER,
    appcommand::GET_ECO,
];

/// How the state is read over HTTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum UpdateMethod {
    /// `AppCommand.xml`. One request reads all zones.
    AppCommand,
    /// Status pages. Receivers without `AppCommand` have them. Two requests
    /// for each zone.
    StatusPages,
}

/// What is known about a receiver after [`AvrBuilder::connect`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// Host that was given.
    pub host: String,
    /// Generation of the receiver.
    pub receiver_type: ReceiverType,
    /// HTTP port that is used for AppCommand and the status pages.
    pub port: u16,
    /// How the state is read.
    pub update_method: UpdateMethod,
    /// Brand. [`Brand::Denon`] if the receiver did not tell.
    pub brand: Brand,
    /// Manufacturer from the device description.
    pub manufacturer: Option<String>,
    /// Model from the device description.
    pub model_name: Option<String>,
    /// Serial number from the device description.
    pub serial_number: Option<String>,
    /// Name that the user gave the receiver.
    pub friendly_name: String,
    /// Version of the communication API.
    pub comm_api_version: Option<String>,
    /// MAC address of the wired network, as 12 hexadecimal digits without
    /// separators.
    pub mac_address: Option<String>,
    /// The receiver supports Dirac Live.
    pub dirac_live: bool,
    /// Zones that [`Avr::update`] reads. The main zone and the others that the
    /// receiver has.
    pub zones: Vec<Zone>,
}

/// Settings of the requests to the web interface. A receiver without it
/// should not slow the caller down.
const WEB_INTERFACE_CONFIG: HttpConfig = HttpConfig {
    connect_timeout: Duration::from_secs(1),
    request_timeout: Duration::from_secs(5),
    max_response_bytes: 256 * 1024,
    idle_timeout: Duration::from_secs(30),
    pacing: Pacing::Fixed {
        interval: Duration::from_millis(50),
    },
};

/// Time allowed for one request of the device description. The receiver
/// sometimes does not answer it, and the description is optional.
const DESCRIPTION_TIMEOUT: Duration = Duration::from_secs(2);

/// Settings for a connection. Start with [`Avr::builder`].
#[derive(Debug, Clone)]
pub struct AvrBuilder {
    host: String,
    http: HttpConfig,
    web_interface: HttpConfig,
    web_interface_port: u16,
    probes: Option<Vec<(ReceiverType, u16)>>,
    description_port: Option<u16>,
    follow_up_read: Option<FollowUpRead>,
    #[cfg(feature = "telnet")]
    telnet: TelnetConfig,
}

/// A receiver that is read over HTTP.
///
/// The state is in a [`watch`] channel. [`Avr::start`] or [`Avr::update`]
/// reads it from the receiver. Everything is `&self`, so one value can be
/// shared between tasks. See the crate docs for the meaning of the prefixes
/// `update_`, `telnet_` and `refresh`.
#[derive(Debug)]
pub struct Avr {
    http: Arc<HttpClient>,
    /// The web interface on its own port.
    web_interface: HttpClient,
    info: Info,
    /// The queries of [`Avr::update`].
    update_commands: Box<[Definition]>,
    /// The request body of `update_commands`.
    update_body: Bytes,
    /// The request body of [`HTTP_ONLY_COMMANDS`].
    #[cfg(feature = "telnet")]
    http_only_body: Bytes,
    /// The request body of [`HTTP_ONLY_WITH_DEVICE_COMMANDS`].
    #[cfg(feature = "telnet")]
    http_only_with_device_body: Bytes,
    /// The request body of [`APPCOMMAND0300_COMMANDS`]. Receivers of the 2016
    /// generation and later have them.
    appcommand0300_body: Bytes,
    /// The receiver does not answer the queries of `AppCommand0300.xml`, so
    /// they are not asked again.
    appcommand0300_missing: AtomicBool,
    /// The last [`Avr::update_signal`] read the signal and the channels with
    /// `AppCommand0300.xml`.
    signal_from_appcommand: AtomicBool,
    channels: Channels,
    /// A start is running, see [`Avr::start_with`].
    starting: AtomicBool,
    #[cfg(feature = "telnet")]
    telnet: Telnet,
    /// The reset key that was given out last.
    reset_keys: ResetKeys,
    /// Commands over HTTP go one after the other.
    command_gate: Mutex<()>,
    /// Reads the values that a command over HTTP changed.
    follow_up: Option<FollowUp>,
}

impl Avr {
    /// Starts the settings for a receiver at `host`.
    #[must_use]
    pub fn builder(host: impl Into<String>) -> AvrBuilder {
        AvrBuilder {
            host: host.into(),
            http: HttpConfig::default(),
            web_interface: WEB_INTERFACE_CONFIG,
            web_interface_port: WEB_INTERFACE_PORT,
            probes: None,
            description_port: None,
            follow_up_read: Some(FollowUpRead::default()),
            #[cfg(feature = "telnet")]
            telnet: TelnetConfig::default(),
        }
    }

    /// What is known about the receiver.
    #[must_use]
    pub const fn info(&self) -> &Info {
        &self.info
    }

    /// Watches the state. It is empty until the first [`Avr::start`] or
    /// [`Avr::update`] has run.
    #[must_use]
    pub fn state(&self) -> watch::Receiver<State> {
        self.channels.watch_state()
    }

    /// Follows each change of the state, with the zone and the value that
    /// changed. A subscriber that is too slow loses the oldest changes and gets
    /// [`broadcast::error::RecvError::Lagged`]. The state is still correct:
    /// read it again with [`Avr::snapshot`] and go on listening.
    #[must_use]
    pub fn changes(&self) -> broadcast::Receiver<Change> {
        self.channels.subscribe_changes()
    }

    /// A copy of the state now.
    #[must_use]
    pub fn snapshot(&self) -> State {
        self.channels.snapshot()
    }

    /// Reads the state of all zones from the receiver over HTTP.
    ///
    /// Watchers are told only if a value changed. To read all values again
    /// later, use [`Avr::refresh`].
    ///
    /// # Errors
    ///
    /// Returns an error if the receiver cannot be reached, or if an answer is
    /// not valid. Values that were read before the error stay in the state.
    pub async fn update(&self) -> Result<(), Error> {
        match self.info.update_method {
            UpdateMethod::AppCommand => self.update_with_appcommand().await,
            UpdateMethod::StatusPages => self.update_with_status_pages().await,
        }
    }

    /// Reads all values again, in the way that fits. If the telnet connection
    /// is open, it asks on telnet, as `Avr::telnet_refresh` does, and reads
    /// over HTTP only what telnet does not report: the names of the zones, and
    /// the dimmer, eco and auto standby if `TelnetConfig::query_groups` has
    /// no `Device` group. If telnet is not open, it does what [`Avr::update`]
    /// does.
    ///
    /// Values from telnet arrive as events after the call returns. With
    /// telnet open both are tried, and the first error is returned. Use
    /// [`Avr::update`] for the first read, before telnet is open. The pages of
    /// the web interface and what plays have their own calls, because they
    /// have their own values.
    ///
    /// # Errors
    ///
    /// The errors of [`Avr::update`] or, if telnet is open, of
    /// `Avr::telnet_refresh` and of the read of the zone names.
    pub async fn refresh(&self) -> Result<(), Error> {
        #[cfg(feature = "telnet")]
        if *self.telnet.watch_status().borrow() == TelnetStatus::Connected {
            let http = self.update_http_only().await;
            return http.and(self.telnet_refresh().await);
        }
        self.update().await
    }

    /// Reads the speaker distances, the crossovers, the channel levels, the
    /// bass settings, how two-channel sources are played and what the XLR
    /// outputs carry from the web interface on port 11080.
    ///
    /// Only these pages have them, and they do not send changes: call it
    /// again to read them again. The values join the telnet values in
    /// [`DeviceState::speaker_setup`](denon_marantz_proto::DeviceState::speaker_setup) and
    /// [`DeviceState::channel_volumes`](denon_marantz_proto::DeviceState::channel_volumes).
    /// The last two are
    /// [`DeviceState::playback_2ch`](denon_marantz_proto::DeviceState::playback_2ch) and
    /// [`DeviceState::xlr_assign`](denon_marantz_proto::DeviceState::xlr_assign).
    ///
    /// The result says what happened to each page. A page that the receiver
    /// does not have, refuses or sends in a form that the library cannot read
    /// is not an error. The channel level page is not asked for while the
    /// state shows that a Dirac ART filter is active, because the receiver
    /// does not answer it then.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiver does not answer in time or the
    /// connection fails, see [`Error::is_unreachable`]. Pages that were read
    /// before stay in the state. A receiver without the port gives this error
    /// after the connect time of [`AvrBuilder::web_interface_config`], one second by
    /// default, on each call.
    pub async fn update_speaker_details(&self) -> Result<SpeakerDetails, Error> {
        let distances = self
            .read_web_interface(
                DISTANCES_PATH,
                SpeakerTable::parse_distances,
                State::apply_speaker_distances,
            )
            .await?;
        let crossovers = self
            .read_web_interface(
                CROSSOVERS_PATH,
                SpeakerTable::parse_crossovers,
                |state, table| state.apply_speaker_crossovers(&table),
            )
            .await?;
        let channel_levels = if self.dirac_art_active() {
            PageStatus::Skipped(SkipReason::DiracArt)
        } else {
            // No second try: a Dirac ART filter makes this page fail each time.
            self.read_page(
                &self.web_interface,
                CHANNEL_LEVELS_PATH,
                SpeakerTable::parse_channel_levels,
                |state, table| state.apply_channel_level_table(&table),
            )
            .await?
        };
        let bass = self
            .read_web_interface(BASS_PATH, BassSettings::parse, State::apply_bass_settings)
            .await?;
        let playback_2ch = self
            .read_web_interface(
                PLAYBACK_2CH_PATH,
                Playback2Ch::parse,
                State::apply_playback_2ch,
            )
            .await?;
        let xlr_assign = self
            .read_web_interface(XLR_ASSIGN_PATH, XlrAssign::parse, State::apply_xlr_assign)
            .await?;
        Ok(SpeakerDetails {
            distances,
            crossovers,
            channel_levels,
            bass,
            playback_2ch,
            xlr_assign,
        })
    }

    /// Reads the physical input of each source, the names of the sources
    /// (default, user and automatic) and the levels of the analog and digital
    /// inputs from the web interface on port 11080.
    ///
    /// Telnet has the names that the user gave, but not the others and not the
    /// inputs. The pages do not send changes: call it again to read them
    /// again. The values are in
    /// [`DeviceState::sources`](denon_marantz_proto::DeviceState::sources).
    ///
    /// The result says what happened to each page. A page that the receiver
    /// does not have or refuses is not an error.
    ///
    /// # Errors
    ///
    /// The same as [`Avr::update_speaker_details`].
    pub async fn update_source_details(&self) -> Result<SourceDetails, Error> {
        let assignments = self
            .read_web_interface(INPUT_ASSIGN_PATH, InputAssignment::parse, |state, list| {
                state.apply_input_assignments(&list)
            })
            .await?;
        let names = self
            .read_web_interface(SOURCE_NAMES_PATH, SourceNames::parse, |state, names| {
                state.apply_web_source_names(&names)
            })
            .await?;
        let levels = self
            .read_web_interface(
                SOURCE_LEVELS_PATH,
                SourceLevels::parse,
                State::apply_source_levels,
            )
            .await?;
        Ok(SourceDetails {
            assignments,
            names,
            levels,
        })
    }

    /// Reads which settings the receiver offers and lets the user change
    /// right now, from the web interface on port 11080.
    ///
    /// This changes with the sound mode, the signal and the Dirac filter, and
    /// the pages do not send changes: call it again to read it again. The
    /// values are in
    /// [`DeviceState::settings_availability`](denon_marantz_proto::DeviceState::settings_availability).
    /// It asks for one page for each group in [`SettingsGroup::ALL`].
    ///
    /// The result says what happened to each group. A group that the receiver
    /// does not have or refuses is not an error.
    ///
    /// # Errors
    ///
    /// The same as [`Avr::update_speaker_details`].
    pub async fn update_settings_availability(&self) -> Result<SettingsAvailabilityDetails, Error> {
        self.read_availability(&SettingsGroup::ALL).await
    }

    /// Reads for each setting of the surround parameters, the HDMI setup and
    /// the video output settings if the receiver lets the user change it now,
    /// from the web interface on port 11080. For example, the dialog control
    /// is not offered in a sound mode that has none.
    ///
    /// A receiver ignores the telnet command for a setting that is not
    /// offered, and sends no answer. The pages do not send changes: call it
    /// again after the sound mode changed. It asks for one page for each group
    /// in [`SettingsGroup::DETAILS`]. The values are in
    /// [`DeviceState::settings_availability`](denon_marantz_proto::DeviceState::settings_availability),
    /// with the groups of [`Avr::update_settings_availability`]. The values of
    /// the pages are not read.
    ///
    /// The result says what happened to each page. A page that the receiver
    /// does not have or refuses is not an error.
    ///
    /// # Errors
    ///
    /// The same as [`Avr::update_speaker_details`].
    pub async fn update_detailed_availability(&self) -> Result<SettingsAvailabilityDetails, Error> {
        self.read_availability(&SettingsGroup::DETAILS).await
    }

    /// Reads the pages of these groups, one after the other.
    async fn read_availability(
        &self,
        groups: &[SettingsGroup],
    ) -> Result<SettingsAvailabilityDetails, Error> {
        self.read_page_list(
            groups,
            SettingsGroup::path,
            GroupAvailability::parse,
            State::apply_settings_availability,
        )
        .await
    }

    /// Reads the video menu from the web interface on port 11080: the
    /// on-screen display, the TV format, the screen saver, the HDMI upscaler,
    /// the HDCP version of each input and the 4K/8K signal format of each
    /// input.
    ///
    /// The pages do not send changes: call it again to read them again. The
    /// values are in
    /// [`DeviceState::video_menu`](denon_marantz_proto::DeviceState::video_menu).
    /// The result says what happened to each page. A page that the receiver
    /// does not have or refuses is not an error.
    ///
    /// # Errors
    ///
    /// The same as [`Avr::update_speaker_details`].
    pub async fn update_video_details(&self) -> Result<VideoMenuDetails, Error> {
        self.read_page_list(
            &VideoMenuPage::ALL,
            VideoMenuPage::path,
            VideoMenuPage::parse,
            State::apply_video_menu,
        )
        .await
    }

    /// Reads the general menu from the web interface on port 11080: the
    /// language, the settings of the firmware update, whether usage data is
    /// sent, and what switches each of the three trigger outputs.
    ///
    /// The pages do not send changes: call it again to read them again. The
    /// values are in
    /// [`DeviceState::general_settings`](denon_marantz_proto::DeviceState::general_settings).
    /// The result says what happened to each page. A page that the receiver
    /// does not have or refuses is not an error.
    ///
    /// # Errors
    ///
    /// The same as [`Avr::update_speaker_details`].
    pub async fn update_general_settings(&self) -> Result<GeneralDetails, Error> {
        self.read_page_list(
            &GeneralPage::ALL,
            GeneralPage::path,
            GeneralPage::parse,
            State::apply_general_settings,
        )
        .await
    }

    /// Reads these pages of the web interface one after the other and puts
    /// them into the state. `parse` reads the page that `page` names.
    async fn read_page_list<P, T>(
        &self,
        pages: &[P],
        path: fn(P) -> &'static str,
        parse: fn(P, &[u8]) -> Result<T, denon_marantz_proto::Error>,
        apply: fn(&mut State, T) -> Option<Change>,
    ) -> Result<PageResults<P>, Error>
    where
        P: Copy + Send + Sync,
        T: Send,
    {
        let mut results = Vec::with_capacity(pages.len());
        for &page in pages {
            let status = self
                .read_web_interface(path(page), move |body| parse(page, body), apply)
                .await?;
            results.push((page, status));
        }
        Ok(PageResults { pages: results })
    }

    /// Reads the settings that telnet does not report from the web interface
    /// on port 11080: HDMI control, the menu lock, firmware updates, when the
    /// network answers and the streaming services.
    ///
    /// The page does not send changes: call it again to read it again. The
    /// values are in
    /// [`DeviceState::system_settings`](denon_marantz_proto::DeviceState::system_settings).
    /// A page that the receiver does not have or refuses is not an error.
    ///
    /// # Errors
    ///
    /// The same as [`Avr::update_speaker_details`].
    pub async fn update_system_settings(&self) -> Result<PageStatus, Error> {
        self.read_web_interface(
            SYSTEM_SETTINGS_PATH,
            SystemSettings::parse,
            State::apply_system_settings,
        )
        .await
    }

    /// Reads what the receiver plays from a status page, for network audio,
    /// HD radio and the tuner.
    ///
    /// The page does not send changes: call it again to read it again. The
    /// values of network audio and HD radio are in
    /// [`DeviceState::now_playing`](denon_marantz_proto::DeviceState::now_playing).
    /// The page of the tuner gives the band and the frequency, which are in
    /// [`DeviceState::tuner`](denon_marantz_proto::DeviceState::tuner) with
    /// the values of telnet. For another source no page is asked for. What
    /// was known in `now_playing` is cleared when the source does not use it.
    /// A receiver that uses HEOS for network audio does not have the pages.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiver cannot be reached, see
    /// [`Error::is_unreachable`].
    pub async fn update_now_playing(&self) -> Result<PageStatus, Error> {
        let result = self.read_now_playing_page().await;
        self.forget_telnet_values(&result);
        result
    }

    async fn read_now_playing_page(&self) -> Result<PageStatus, Error> {
        match self.playing_kind() {
            Some(PlayingKind::NetAudio) => {
                self.read_page(
                    &self.http,
                    NET_AUDIO_STATUS_PATH,
                    NowPlaying::parse_net_audio,
                    State::apply_now_playing,
                )
                .await
            }
            Some(PlayingKind::HdRadio) => {
                self.read_page(
                    &self.http,
                    HD_RADIO_STATUS_PATH,
                    NowPlaying::parse_hd_radio,
                    State::apply_now_playing,
                )
                .await
            }
            Some(PlayingKind::Tuner) => {
                self.clear_now_playing();
                self.read_page(
                    &self.http,
                    TUNER_STATUS_PATH,
                    TunerPage::parse,
                    State::apply_tuner_page,
                )
                .await
            }
            _ => {
                self.clear_now_playing();
                Ok(PageStatus::Skipped(SkipReason::NoPlayingSource))
            }
        }
    }

    fn clear_now_playing(&self) {
        self.channels
            .modify(|state, notify| notify(state.apply_now_playing(NowPlaying::default())));
    }

    /// Asks for what plays. It picks the way. If the
    /// telnet connection is open it asks on telnet, see
    /// `Avr::telnet_refresh_now_playing`. If not, it reads the status page, see
    /// [`Avr::update_now_playing`].
    ///
    /// Either way the values reach
    /// [`DeviceState::now_playing`](denon_marantz_proto::DeviceState::now_playing)
    /// (and the tuner state for the tuner) and watchers of
    /// [`Avr::changes`] are told. Over telnet that happens after the call
    /// returns, when the receiver answers. Nothing asks again by itself: call
    /// it again to see changes.
    ///
    /// # Errors
    ///
    /// The errors of the way that was used.
    pub async fn refresh_now_playing(&self) -> Result<(), Error> {
        #[cfg(feature = "telnet")]
        if *self.telnet.watch_status().borrow() == TelnetStatus::Connected {
            return self.telnet_refresh_now_playing().await;
        }
        self.update_now_playing().await.map(|_| ())
    }

    /// The address of the picture of what plays, if the source has one.
    ///
    /// Network audio has the cover of the title. The address changes with the
    /// title, the artist and the album, so a client that keeps pictures sees a
    /// new one. The tuner and HD radio have one picture for all stations. The
    /// receiver does not say if it has a cover, so the address can lead to a
    /// picture that is empty.
    #[must_use]
    pub fn artwork_url(&self) -> Option<String> {
        use std::hash::{Hash, Hasher};

        let host = &self.info.host;
        let port = self.info.port;
        match self.playing_kind()? {
            PlayingKind::NetAudio => {
                let mut hasher = std::hash::DefaultHasher::new();
                {
                    let state = self.channels.watch_state();
                    let state = state.borrow();
                    let now = &state.device().now_playing;
                    (&now.title, &now.artist, &now.album).hash(&mut hasher);
                }
                Some(format!(
                    "http://{host}:{port}{}?{}",
                    denon_marantz_proto::COVER_ARTWORK_PATH,
                    hasher.finish()
                ))
            }
            _ => Some(format!(
                "http://{host}:{port}{}",
                denon_marantz_proto::STATIC_ARTWORK_PATH
            )),
        }
    }

    /// What the source of the main zone plays, from its name or its telnet
    /// name. A source that has a name of the user is found by that name.
    fn playing_kind(&self) -> Option<PlayingKind> {
        let state = self.channels.watch_state();
        let state = state.borrow();
        let source = state.zone(Zone::Main).source.as_deref()?;
        denon_marantz_proto::source::playing_kind(source).or_else(|| {
            state
                .device()
                .sources
                .as_slice()
                .iter()
                .find(|info| info.name.as_deref() == Some(source))
                .and_then(|info| denon_marantz_proto::source::playing_kind(&info.code))
        })
    }

    /// Returns true if the state shows that a Dirac ART filter is active.
    fn dirac_art_active(&self) -> bool {
        self.channels
            .watch_state()
            .borrow()
            .device()
            .dirac_art_active()
            == Some(true)
    }

    /// Reads one web interface page into the state. Returns an error only if
    /// the receiver is not reachable. A page that fails with a server error
    /// or an unreadable answer is asked for once more: the receiver does this
    /// at random, about once in 90 requests, and answers the second time.
    async fn read_web_interface<T>(
        &self,
        path: &str,
        parse: impl Fn(&[u8]) -> Result<T, denon_marantz_proto::Error> + Send + Sync,
        apply: impl Fn(&mut State, T) -> Option<Change> + Send + Sync,
    ) -> Result<PageStatus, Error> {
        let status = self
            .read_page(&self.web_interface, path, &parse, &apply)
            .await?;
        if matches!(status, PageStatus::Refused(500..) | PageStatus::Unreadable) {
            return self
                .read_page(&self.web_interface, path, &parse, &apply)
                .await;
        }
        Ok(status)
    }

    /// Reads one page from a client into the state. Returns an error only if
    /// the receiver is not reachable.
    async fn read_page<T>(
        &self,
        client: &HttpClient,
        path: &str,
        parse: impl FnOnce(&[u8]) -> Result<T, denon_marantz_proto::Error>,
        apply: impl FnOnce(&mut State, T) -> Option<Change>,
    ) -> Result<PageStatus, Error> {
        match client.get_parsed(path, parse).await {
            Ok(value) => {
                self.channels
                    .modify(|state, notify| notify(apply(state, value)));
                Ok(PageStatus::Read)
            }
            Err(error) if error.is_unreachable() => Err(error),
            Err(error) => {
                tracing::debug!(host = %self.info.host, %error, "web interface page not read");
                Ok(match error {
                    Error::Status { status: 404, .. } => PageStatus::NotFound,
                    Error::Status { status, .. } => PageStatus::Refused(status),
                    Error::Forbidden { .. } => PageStatus::Refused(403),
                    _ => PageStatus::Unreadable,
                })
            }
        }
    }

    /// Opens the telnet connection and keeps it open.
    ///
    /// Events from the receiver then change the state at once. If the
    /// connection is lost, a task connects again. Does nothing while the
    /// task runs, also while it connects again. Older receivers allow one
    /// telnet connection, so another client can take it away; the task takes
    /// it back.
    ///
    /// # Errors
    ///
    /// Returns an error if the first connection fails. Nothing keeps trying
    /// then.
    #[cfg(feature = "telnet")]
    pub async fn telnet_connect(&self) -> Result<(), Error> {
        self.telnet
            .connect(&self.info.host, &self.channels, None)
            .await
            .map(|_| ())
    }

    /// Closes the telnet connection and stops the task that keeps it open.
    #[cfg(feature = "telnet")]
    pub async fn telnet_disconnect(&self) {
        self.telnet.disconnect().await;
    }

    /// Watches the telnet connection. It is disconnected until
    /// [`Avr::telnet_connect`] has worked, and while the task tries to
    /// connect again.
    #[cfg(feature = "telnet")]
    #[must_use]
    pub fn telnet_status(&self) -> watch::Receiver<TelnetStatus> {
        self.telnet.watch_status()
    }

    /// Sends a command on telnet.
    ///
    /// Commands wait for each other and for the spacing between writes (see
    /// [`TelnetConfig::command_interval`]). They go before the queries that
    /// fill the state. This returns when the command is written. That does not
    /// mean that the receiver did it: its answer is an event. If the command
    /// is waiting when the future is dropped, it is still sent.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotConnected`] if the telnet connection is not open.
    /// Nothing is queued then, so a command is never sent late. Returns
    /// [`Error::InvalidCommand`] if the command has text that cannot be sent.
    /// If the connection ends while the command is being written, this also
    /// returns [`Error::NotConnected`], and the receiver may have got the line.
    #[cfg(feature = "telnet")]
    pub async fn telnet_send(&self, zone: Zone, command: Command<'_>) -> Result<(), Error> {
        self.telnet
            .send(command::encode(self.info.brand, zone, &command)?)
            .await
    }

    /// Asks the receiver on telnet what it plays. The answer comes as events
    /// that change [`DeviceState::now_playing`](denon_marantz_proto::DeviceState::now_playing)
    /// or the tuner state.
    ///
    /// It asks for the lines of network audio, the HD radio information or the
    /// tuner frequency and name, for the source of the main zone, and does
    /// nothing for another source. Ask again to see changes. A receiver that
    /// uses HEOS for network audio does not answer.
    ///
    /// # Errors
    ///
    /// The same as [`Avr::telnet_send`].
    #[cfg(feature = "telnet")]
    pub async fn telnet_refresh_now_playing(&self) -> Result<(), Error> {
        match self.playing_kind() {
            Some(PlayingKind::NetAudio) => self.telnet_send(Zone::Main, Command::Raw("NSE")).await,
            Some(PlayingKind::HdRadio) => self.telnet_send(Zone::Main, Command::Raw("HD?")).await,
            Some(PlayingKind::Tuner) => {
                for query in denon_marantz_proto::TUNER_QUERIES {
                    self.telnet_send(Zone::Main, Command::Raw(query)).await?;
                }
                self.telnet_send(Zone::Main, Command::Raw("TFANNAME?"))
                    .await
            }
            _ => Ok(()),
        }
    }

    /// Sends a command to the receiver.
    ///
    /// It goes on telnet if the connection is open, as `Avr::telnet_send`
    /// does, and over HTTP otherwise, as [`Avr::http_send`] does. All typed
    /// controls ([`Avr::zone`] and the others) use this call, so they work
    /// without telnet. A caller that does not want telnet never opens it.
    ///
    /// # Errors
    ///
    /// The errors of `Avr::telnet_send` if telnet is used, and of
    /// [`Avr::http_send`] otherwise. A query (a command that asks for a value)
    /// gives [`Error::QueryNeedsTelnet`] whenever telnet is not connected, also
    /// while it connects again. Ask again when `Avr::telnet_status` is
    /// connected.
    pub async fn send(&self, zone: Zone, command: Command<'_>) -> Result<(), Error> {
        #[cfg(feature = "telnet")]
        if *self.telnet.watch_status().borrow() == TelnetStatus::Connected {
            return self.telnet_send(zone, command).await;
        }
        self.http_send(zone, command).await
    }

    /// Sends a command over HTTP, with the page `formiPhoneAppDirect.xml`
    /// of the receiver.
    ///
    /// Commands wait for each other, in the order of the calls, and for the
    /// spacing of the HTTP requests (see [`AvrBuilder::http_config`]). This
    /// returns when the receiver has answered the request. The answer does not
    /// say if the receiver did the command, as on telnet. The receiver does not
    /// send events over HTTP, so a command that changes a value of
    /// [`Avr::update`] makes a read of these values later, see
    /// [`AvrBuilder::follow_up_read`]. The values of other settings change
    /// when they are read.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`] if the command has text that cannot be
    /// sent. Returns [`Error::QueryNeedsTelnet`] for a query (a command that asks
    /// for a value): the receiver answers it on telnet only. Returns the
    /// errors of an HTTP request if it fails.
    pub async fn http_send(&self, zone: Zone, command: Command<'_>) -> Result<(), Error> {
        let line = command::encode(self.info.brand, zone, &command)?;
        if command::is_query(&line) {
            return Err(Error::QueryNeedsTelnet);
        }
        {
            // The lock keeps the order of the callers.
            let _turn = self.command_gate.lock().await;
            self.http.get(&command::http_path(&line)).await?;
        }
        if let (Some(follow_up), Some(reads)) = (&self.follow_up, Reads::after(&command)) {
            follow_up.note(reads);
        }
        Ok(())
    }

    /// Controls one zone with typed methods. The `Result` is for
    /// the zone: not every receiver has zones 2 and 3.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoSuchZone`] if the receiver does not have the zone.
    pub fn zone(&self, zone: Zone) -> Result<ZoneControl<'_>, Error> {
        if self.info.zones.contains(&zone) {
            Ok(ZoneControl::new(self, zone))
        } else {
            Err(Error::NoSuchZone(zone))
        }
    }

    /// Controls bass, treble and tone control with typed methods.
    #[must_use]
    pub const fn tone(&self) -> ToneControl<'_> {
        ToneControl::new(self)
    }

    /// Controls the Audyssey settings with typed methods.
    #[must_use]
    pub const fn audyssey(&self) -> Audyssey<'_> {
        Audyssey::new(self)
    }

    /// Controls the sound settings with typed methods.
    #[must_use]
    pub const fn sound(&self) -> SoundControl<'_> {
        SoundControl::new(self)
    }

    pub(crate) const fn reset_keys(&self) -> &ResetKeys {
        &self.reset_keys
    }

    /// Controls the settings of the whole receiver with typed
    /// methods.
    #[must_use]
    pub const fn device(&self) -> DeviceControl<'_> {
        DeviceControl::new(self)
    }

    /// Controls the tuner with typed methods.
    #[must_use]
    pub const fn tuner(&self) -> TunerControl<'_> {
        TunerControl::new(self)
    }

    /// Controls the video settings with typed methods.
    #[must_use]
    pub const fn video(&self) -> VideoControl<'_> {
        VideoControl::new(self)
    }

    /// Controls Dirac Live with typed methods. The `Result` is for
    /// the receiver: not every receiver has Dirac Live.
    ///
    /// # Errors
    ///
    /// Returns [`Error::DiracNotSupported`] if the receiver does not support
    /// Dirac Live.
    pub const fn dirac(&self) -> Result<Dirac<'_>, Error> {
        if self.info.dirac_live {
            Ok(Dirac::new(self))
        } else {
            Err(Error::DiracNotSupported)
        }
    }

    /// Waits until the receiver has answered a last query that is sent after
    /// the telnet queries that wait now. The receiver handles lines in order,
    /// so the answers to the earlier queries are in the state when this
    /// returns. A value that is still empty is probably one that this receiver
    /// does not answer. The library cannot check this: an event that looks like
    /// the answer, or a query that the receiver drops, can leave a value for
    /// later. Queries that are added later are not covered. See also
    /// [`Started::all_synced`].
    ///
    /// Dropping the future is safe. Several calls at once each wait for their
    /// own query, except that a power event or the answer to the keep-alive
    /// query can end one wait early.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotConnected`] if the telnet connection is not open or
    /// ends before the answer, and an error for which [`Error::is_timeout`] is
    /// true if there is no answer within the keep-alive time.
    #[cfg(feature = "telnet")]
    pub async fn telnet_sync(&self) -> Result<(), Error> {
        self.telnet.sync().await
    }

    /// As [`Avr::telnet_sync`], with this limit for the answer.
    #[cfg(feature = "telnet")]
    pub(crate) async fn telnet_sync_within(&self, limit: Duration) -> Result<(), Error> {
        self.telnet.sync_within(limit).await
    }

    /// Asks the receiver for its state again on telnet. The answers are
    /// events, and events that repeat the last value are shown again. The
    /// names of the zones are not on telnet: [`Avr::refresh`] reads them too.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotConnected`] if the telnet connection is not open.
    /// A request that arrives as the connection ends can be dropped with `Ok`.
    #[cfg(feature = "telnet")]
    pub async fn telnet_refresh(&self) -> Result<(), Error> {
        self.telnet.refresh().await
    }

    /// Asks the receiver on telnet for exactly these values, for example the
    /// values that depend on one that changed. The queries wait in the same
    /// queue as the others, with the same spacing, and a query that already
    /// waits is not added again. The answers are events, and events that
    /// repeat the last value are shown again.
    ///
    /// A [`Query`](crate::Query) gives the query of the brand of the receiver. [`Ask::Raw`]
    /// asks a query as text.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRequest`] if a value has no query for the brand
    /// of the receiver, or if a raw query does not end with `?`. Returns
    /// [`Error::InvalidCommand`] if the text of a raw query is empty or has a
    /// control character. Returns [`Error::NotConnected`] if the telnet
    /// connection is not open. Nothing is asked if there is an error.
    #[cfg(feature = "telnet")]
    pub async fn telnet_ask(&self, what: &[Ask<'_>]) -> Result<(), Error> {
        let brand = self.info.brand;
        let mut queries: Vec<std::borrow::Cow<'static, str>> = Vec::with_capacity(what.len());
        for ask in what {
            queries.push(match *ask {
                Ask::Query(query) => {
                    std::borrow::Cow::Borrowed(query.text(brand).ok_or_else(|| {
                        Error::InvalidRequest(format!("{query:?} has no query for {brand:?}"))
                    })?)
                }
                Ask::Raw(text) => {
                    let mut line = command::encode(brand, Zone::Main, &Command::Raw(text))?;
                    if !text.ends_with('?') {
                        return Err(Error::InvalidRequest(format!(
                            "query {text:?} does not end with ?"
                        )));
                    }
                    // The line ends with the carriage return that the queue adds.
                    line.pop();
                    std::borrow::Cow::Owned(line)
                }
            });
        }
        self.telnet.ask(queries).await
    }

    /// Asks the receiver on telnet for one group of values, for example the
    /// signal information. The answers are events, and events that repeat the
    /// last value are shown again. A group that the connection does not fetch
    /// at connect (see [`TelnetConfig::query_groups`]) stays empty until this
    /// is called.
    ///
    /// The receiver does not send the values that depend on a changed value,
    /// for example the Dirac Live filters of another speaker preset. A caller
    /// that shows such values asks for them when the event arrives, with
    /// [`Avr::telnet_ask`], which asks for exactly these values.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotConnected`] if the telnet connection is not open.
    #[cfg(feature = "telnet")]
    pub async fn telnet_refresh_group(&self, group: QueryGroup) -> Result<(), Error> {
        self.telnet.refresh_group(group).await
    }

    /// Follows the events from the receiver, as they come on telnet. Repeated
    /// events that a query causes are left out. A subscriber that is too slow
    /// loses the oldest events and gets [`broadcast::error::RecvError::Lagged`].
    /// Go on listening after it, and read the state with [`Avr::snapshot`].
    #[cfg(feature = "telnet")]
    #[must_use]
    pub fn telnet_events(&self) -> broadcast::Receiver<TelnetEvent> {
        self.telnet.subscribe_events()
    }

    async fn update_with_appcommand(&self) -> Result<(), Error> {
        self.read_appcommand(
            Endpoint::AppCommand,
            &self.update_commands,
            self.update_body.clone(),
        )
        .await
    }

    /// Reads the video and audio signal, the channels and the Audyssey
    /// settings from the receiver with one request to `AppCommand0300.xml`.
    ///
    /// This is faster than the pages of the web interface and the telnet
    /// queries for the same values. It gives the
    /// resolution that the receiver gets and sends, the kind of the audio
    /// signal and of the sound, the sampling rate, and the channels of the
    /// signal and of the speakers. HDR, the pixel depth, the color space and
    /// the FRL rate are not in it: [`Avr::refresh_signal`] reads them from
    /// the web interface. The values are in
    /// [`DeviceState::signal`](denon_marantz_proto::DeviceState::signal) and,
    /// for Audyssey, in the fields of [`DeviceState`](denon_marantz_proto::DeviceState).
    ///
    /// Only receivers of the 2016 generation and later have it. The result is
    /// [`PageStatus::NotFound`] for another receiver and for a receiver that
    /// answers 404, and then it is not asked again. Another refusal or an
    /// answer that cannot be read gives [`PageStatus::Refused`] or
    /// [`PageStatus::Unreadable`], and the next call asks again. An item of the
    /// answer with an error leaves its values as they are. A receiver that is
    /// not calibrated with Audyssey answers with blank Audyssey values, which
    /// count as unknown.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiver does not answer in time or the
    /// connection fails, see [`Error::is_unreachable`].
    pub async fn update_signal(&self) -> Result<PageStatus, Error> {
        if self.info.receiver_type != ReceiverType::AvrX2016
            || self.appcommand0300_missing.load(Ordering::Relaxed)
        {
            return Ok(PageStatus::NotFound);
        }
        let result = self
            .http
            .post_parsed(
                Endpoint::AppCommand0300.path(),
                self.appcommand0300_body.clone(),
                |body| {
                    let response = Response::parse(body, APPCOMMAND0300_COMMANDS.len())?;
                    let mut complete = true;
                    self.channels.modify(|state, notify| {
                        for (definition, item) in
                            APPCOMMAND0300_COMMANDS.iter().zip(response.items())
                        {
                            // An error item has no values, and must not clear
                            // what the state has.
                            if item.is_error() {
                                complete = false;
                                continue;
                            }
                            definition.extract(&item, Zone::Main, &mut |field, value| {
                                notify(state.apply_field(Zone::Main, field, value));
                            });
                            if *definition == appcommand::GET_INPUT_SIGNAL {
                                notify(state.apply_channel_item(true, &item));
                            } else if *definition == appcommand::GET_ACTIVE_SPEAKER {
                                notify(state.apply_channel_item(false, &item));
                            }
                        }
                    });
                    Ok(complete)
                },
            )
            .await;
        match result {
            Ok(complete) => {
                // The start asks telnet again for what an error item left out.
                self.signal_from_appcommand
                    .store(complete, Ordering::Relaxed);
                self.forget_telnet_values(&Ok(PageStatus::Read));
                Ok(PageStatus::Read)
            }
            // A timeout or a network error can pass.
            Err(error) if error.is_unreachable() => Err(error),
            Err(error) => {
                tracing::debug!(host = %self.info.host, %error, "AppCommand0300 not read");
                Ok(match error {
                    // Only a missing file means that the receiver does not have it.
                    Error::Status { status: 404, .. } => {
                        self.appcommand0300_missing.store(true, Ordering::Relaxed);
                        PageStatus::NotFound
                    }
                    Error::Status { status, .. } => PageStatus::Refused(status),
                    Error::Forbidden { .. } => PageStatus::Refused(403),
                    _ => PageStatus::Unreadable,
                })
            }
        }
    }

    /// Reads only what telnet does not report: the names of the zones, and the
    /// dimmer, eco and auto standby if telnet does not fetch the `Device`
    /// group. A receiver that uses the status pages does not have them.
    #[cfg(feature = "telnet")]
    async fn update_http_only(&self) -> Result<(), Error> {
        match self.info.update_method {
            UpdateMethod::AppCommand => {
                let (commands, body) = if self.telnet.fetches(QueryGroup::Device) {
                    (&HTTP_ONLY_COMMANDS[..], &self.http_only_body)
                } else {
                    (
                        &HTTP_ONLY_WITH_DEVICE_COMMANDS[..],
                        &self.http_only_with_device_body,
                    )
                };
                self.read_appcommand(Endpoint::AppCommand, commands, body.clone())
                    .await
            }
            UpdateMethod::StatusPages => Ok(()),
        }
    }

    /// Sends `body`, the request for `commands`, and puts the answer into the
    /// state.
    async fn read_appcommand(
        &self,
        endpoint: Endpoint,
        commands: &[Definition],
        body: Bytes,
    ) -> Result<(), Error> {
        read_appcommand(
            &self.http,
            &self.channels,
            &self.info.zones,
            endpoint,
            commands,
            body,
        )
        .await
    }

    async fn update_with_status_pages(&self) -> Result<(), Error> {
        read_status_pages(&self.http, &self.channels, &self.info.zones).await
    }
}

/// Sends `body`, the request for `commands`, and puts the answer into the
/// state of `channels`.
async fn read_appcommand(
    http: &HttpClient,
    channels: &Channels,
    zones: &[Zone],
    endpoint: Endpoint,
    commands: &[Definition],
    body: Bytes,
) -> Result<(), Error> {
    http.post_parsed(endpoint.path(), body, |body| {
        let response = Response::parse(body, commands.len())?;
        channels.modify(|state, notify| {
            for &zone in zones {
                response.extract(commands, zone, |field, value| {
                    notify(state.apply_field(zone, field, value));
                });
            }
            // The source lists are for the whole receiver.
            for (definition, item) in commands.iter().zip(response.items()) {
                if *definition == appcommand::GET_RENAME_SOURCE {
                    item.renamed_sources(|name, rename| {
                        notify(state.apply_renamed_source(name, rename));
                    });
                } else if *definition == appcommand::GET_DELETED_SOURCE {
                    item.source_use(|name, value| {
                        notify(state.apply_source_use(name, value));
                    });
                }
            }
        });
        Ok(())
    })
    .await
}

/// Reads the status pages of each zone into the state of `channels`.
async fn read_status_pages(
    http: &HttpClient,
    channels: &Channels,
    zones: &[Zone],
) -> Result<(), Error> {
    for &zone in zones {
        let status = page(http, status_page::status_path(zone)).await?;
        let zone_page = page(http, status_page::zone_path(zone)).await?;
        let (status, zone_page) = (parse_page(status.as_ref()), parse_page(zone_page.as_ref()));
        channels.modify(|state, notify| {
            status_page::extract(
                status.as_ref(),
                zone_page.as_ref(),
                zone,
                &mut |field, value| {
                    notify(state.apply_field(zone, field, value));
                },
            );
        });
    }
    Ok(())
}

impl AvrBuilder {
    /// Sets timeouts, pacing and limits of the HTTP requests.
    #[must_use]
    pub const fn http_config(mut self, config: HttpConfig) -> Self {
        self.http = config;
        self
    }

    /// Sets timeouts, pacing and limits of the requests to the web interface
    /// pages on port 11080. The default waits 1 second to connect and 5
    /// seconds for an answer, so a receiver without these pages costs little
    /// time.
    #[must_use]
    pub const fn web_interface_config(mut self, config: HttpConfig) -> Self {
        self.web_interface = config;
        self
    }

    /// Sets when the values that a command over HTTP changed are read, or
    /// `None` for no read. The default is [`FollowUpRead::default`]. A caller
    /// that reads the state in its own way can turn it off.
    #[must_use]
    pub const fn follow_up_read(mut self, config: Option<FollowUpRead>) -> Self {
        self.follow_up_read = config;
        self
    }

    /// Sets the port of the web interface. The default is 11080.
    #[must_use]
    pub const fn web_interface_port(mut self, port: u16) -> Self {
        self.web_interface_port = port;
        self
    }

    /// Sets the types and ports to try when identifying the receiver, newest
    /// type first. The default is the usual ports of each generation. Use this
    /// if the AppCommand port is not a usual one, for example behind a proxy.
    #[must_use]
    pub fn probes(mut self, probes: &[(ReceiverType, u16)]) -> Self {
        self.probes = Some(probes.to_vec());
        self
    }

    /// Sets the port of the device description. The default depends on the
    /// type of the receiver.
    #[must_use]
    pub const fn description_port(mut self, port: u16) -> Self {
        self.description_port = Some(port);
        self
    }

    /// Sets the telnet connection: port, timeouts and how soon to connect
    /// again.
    #[cfg(feature = "telnet")]
    #[must_use]
    pub const fn telnet_config(mut self, config: TelnetConfig) -> Self {
        self.telnet = config;
        self
    }

    /// Identifies the receiver and reads what it tells about itself.
    ///
    /// The state is empty until [`Avr::update`] runs.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiver cannot be reached. A missing or
    /// unusable device description is not an error: the brand is then
    /// [`Brand::Denon`] and the model is not known.
    pub async fn connect(self) -> Result<Avr, Error> {
        #[cfg(feature = "telnet")]
        self.telnet.check()?;
        let Identified {
            receiver_type,
            port,
            device_info,
        } = match &self.probes {
            Some(probes) => identify_on(&self.host, probes, self.http).await?,
            None => identify(&self.host, self.http).await?,
        };
        let http = Arc::new(HttpClient::new(&self.host, port, self.http)?);
        let description = self.description(receiver_type).await?;
        let (update_method, friendly_name) = detect_update_method(&http, receiver_type).await?;

        let brand = description
            .as_ref()
            .map_or_else(|| Brand::from_manufacturer(None), DeviceDescription::brand);
        let friendly_name = friendly_name
            .or_else(|| {
                description
                    .as_ref()
                    .and_then(|d| owned(d.friendly_name.as_ref()))
            })
            .unwrap_or_else(|| format!("{} {DEFAULT_NAME}", brand.name()));

        let zone_count =
            usize::from(device_info.device_zones.unwrap_or(1)).clamp(1, Zone::ALL.len());
        let info = Info {
            host: self.host,
            receiver_type,
            port,
            update_method,
            brand,
            manufacturer: description.as_ref().map(|d| d.manufacturer.to_string()),
            model_name: description
                .as_ref()
                .and_then(|d| owned(d.model_name.as_ref())),
            serial_number: description
                .as_ref()
                .and_then(|d| owned(d.serial_number.as_ref())),
            friendly_name,
            comm_api_version: owned(device_info.comm_api_version.as_ref()),
            mac_address: owned(device_info.mac_address.as_ref()),
            dirac_live: device_info.dirac_live,
            zones: Zone::ALL[..zone_count].to_vec(),
        };
        tracing::info!(host = %info.host, name = %info.friendly_name, method = ?info.update_method, "connected");

        let mut update_commands = UPDATE_COMMANDS.to_vec();
        if receiver_type == ReceiverType::AvrX2016 {
            update_commands.push(appcommand::GET_TONE_CONTROL);
        }
        let commands: Vec<Cmd<'_>> = update_commands
            .iter()
            .map(|definition| definition.cmd)
            .collect();
        let update_body = request_body(&commands)?;
        let single_zone = device_info.device_zones == Some(1);
        let channels = Channels::new(State::new(single_zone));
        let follow_up = FollowUp::start(self.follow_up_read, &http, &channels, &info)?;
        Ok(Avr {
            http,
            web_interface: HttpClient::new(
                &info.host,
                self.web_interface_port,
                self.web_interface,
            )?,
            info,
            update_commands: update_commands.into_boxed_slice(),
            update_body,
            #[cfg(feature = "telnet")]
            http_only_body: request_body(&HTTP_ONLY_COMMANDS.map(|definition| definition.cmd))?,
            #[cfg(feature = "telnet")]
            http_only_with_device_body: request_body(
                &HTTP_ONLY_WITH_DEVICE_COMMANDS.map(|definition| definition.cmd),
            )?,
            appcommand0300_body: request_body(
                &APPCOMMAND0300_COMMANDS.map(|definition| definition.cmd),
            )?,
            appcommand0300_missing: AtomicBool::new(false),
            signal_from_appcommand: AtomicBool::new(false),
            channels,
            starting: AtomicBool::new(false),
            #[cfg(feature = "telnet")]
            telnet: Telnet::new(self.telnet, brand, single_zone),
            reset_keys: ResetKeys::default(),
            command_gate: Mutex::new(()),
            follow_up,
        })
    }

    /// Reads the device description, with one more try. `None` if the receiver
    /// has no usable one. The receiver was identified before, so a failure here
    /// is not an error: the description only adds the brand and the model.
    async fn description(
        &self,
        receiver_type: ReceiverType,
    ) -> Result<Option<DeviceDescription<'static>>, Error> {
        let port = self
            .description_port
            .unwrap_or_else(|| receiver_type.description_port());
        let http = HttpClient::new(
            &self.host,
            port,
            self.http.with_request_timeout(DESCRIPTION_TIMEOUT),
        )?;
        let mut last_error = None;
        for _ in 0..2 {
            match http
                .get_parsed(receiver_type.description_path(), |body| {
                    DeviceDescription::parse(body).map(DeviceDescription::into_owned)
                })
                .await
            {
                Ok(description) => return Ok(Some(description)),
                Err(error) => last_error = Some(error),
            }
        }
        if let Some(error) = last_error {
            tracing::warn!(host = %self.host, %error, "no device description, assuming a Denon receiver");
        }
        Ok(None)
    }
}

/// Finds how to read the state, and the friendly name that comes with it.
///
/// AVR-X receivers answer `AppCommand.xml`, unless the firmware lacks it.
/// Then, and for all other receivers, the status pages are used.
async fn detect_update_method(
    http: &HttpClient,
    receiver_type: ReceiverType,
) -> Result<(UpdateMethod, Option<String>), Error> {
    if receiver_type != ReceiverType::Avr {
        let command = appcommand::GET_FRIENDLY_NAME;
        let answer = http
            .post_parsed(
                Endpoint::AppCommand.path(),
                request_body(&[command.cmd])?,
                |body| {
                    let response = Response::parse(body, 1)?;
                    let mut name = None;
                    response.extract(&[command], Zone::Main, |field, value| {
                        if field == Field::FriendlyName {
                            name = value
                                .map(str::trim)
                                .filter(|v| !v.is_empty())
                                .map(str::to_owned);
                        }
                    });
                    Ok(name)
                },
            )
            .await;
        match answer {
            Ok(name) => return Ok((UpdateMethod::AppCommand, name)),
            Err(error) if error.is_unreachable() => return Err(error),
            Err(error) => {
                tracing::info!(%error, "AppCommand is not supported, using the status pages");
            }
        }
    }
    let name = page(http, status_page::zone_path(Zone::Main))
        .await?
        .as_ref()
        .and_then(|body| parse_page(Some(body)))
        .and_then(|page| status_page::friendly_name(&page));
    Ok((UpdateMethod::StatusPages, name))
}

/// The request body for these commands.
fn request_body(commands: &[Cmd<'_>]) -> Result<Bytes, Error> {
    let mut body = Vec::new();
    write_body(commands, &mut body).map_err(|source| Error::Protocol {
        request: "building the AppCommand request".to_owned(),
        source,
    })?;
    Ok(Bytes::from(body))
}

/// Copies optional text.
fn owned(value: Option<&std::borrow::Cow<'_, str>>) -> Option<String> {
    value.map(ToString::to_string)
}

/// Reads a status page. `None` if the receiver has no such page. An error
/// means that the receiver cannot be reached.
async fn page(http: &HttpClient, path: &str) -> Result<Option<Bytes>, Error> {
    match http.get(path).await {
        Ok(body) => Ok(Some(body)),
        Err(error) if error.is_unreachable() => Err(error),
        Err(error) => {
            tracing::debug!(%error, "status page not available");
            Ok(None)
        }
    }
}

/// Checks a status page. An error page counts as no page.
fn parse_page(body: Option<&Bytes>) -> Option<Page<'_>> {
    Page::parse(body?)
        .inspect_err(|error| tracing::debug!(%error, "status page is not usable"))
        .ok()
}
