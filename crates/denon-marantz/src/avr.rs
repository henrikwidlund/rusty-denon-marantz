//! A receiver: identification, state and updates over HTTP.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::time::Duration;

use bytes::Bytes;
use denon_marantz_proto::appcommand::{
    self, Cmd, Definition, Endpoint, Field, Response, SettingsPart, settings_request_asks,
    write_body,
};
use denon_marantz_proto::status_page::{self, Page};
use denon_marantz_proto::telnet::{
    Command, Message, Query, QueryGroup, SignalPart, query_for_command,
};
use denon_marantz_proto::{
    BASS_PATH, BassSettings, Brand, CHANNEL_LEVELS_PATH, CROSSOVERS_PATH, Change, DISTANCES_PATH,
    DeviceDescription, DeviceFeatures, DeviceRanges, GeneralPage, GroupAvailability,
    HD_RADIO_STATUS_PATH, INPUT_ASSIGN_PATH, InputAssignment, ListedQueries, NET_AUDIO_STATUS_PATH,
    NowPlaying, PLAYBACK_2CH_PATH, Playback2Ch, PlayingKind, ReceiverType, SOURCE_LEVELS_PATH,
    SOURCE_NAMES_PATH, SYSTEM_SETTINGS_PATH, SettingsGroup, SourceLevels, SourceNames,
    SpeakerTable, State, SystemSettings, TUNER_STATUS_PATH, TunerInfo, TunerPage, VideoMenuPage,
    WEB_INTERFACE_PORT, XLR_ASSIGN_PATH, XlrAssign, Zone,
};
use denon_marantz_proto::{
    MAX_QUICK_SELECT, NetworkAudioCommand, SpecFeature, SpecGeneration, ZoneSettingValue,
};
use tokio::sync::{broadcast, watch};
use tracing::{Instrument, Span};

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
#[cfg(feature = "heos")]
use crate::heos_link::{HeosLink, HeosOwner, HeosPlayer, HeosPlayerStatus};
use crate::http::{HttpClient, HttpConfig};
use crate::identify::{Identified, TypeProof, identify, identify_on};
use crate::pacer::Pacing;
use crate::priority::Priority;
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
pub use start::{
    HeosStart, InfoCheck, StartOptions, StartPages, StartReport, StartStopper, Started,
    TelnetStart, TelnetSync,
};

/// Name of a receiver that has none.
const DEFAULT_NAME: &str = "AVR";

/// Queries that fill the state. The answer has one item for each, in this order.
/// [`appcommand::GET_TONE_CONTROL`] is last, so that it can be left out for a
/// receiver that does not answer it.
const UPDATE_COMMANDS: [Definition; 13] = [
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
    appcommand::GET_TONE_CONTROL,
];

/// [`UPDATE_COMMANDS`] without tone control.
const UPDATE_COMMANDS_WITHOUT_TONE: &[Definition] = UPDATE_COMMANDS.split_last().unwrap().1;

/// The queries of `AppCommand0300.xml` that [`Avr::update_signal_and_settings`]
/// can send: the Audyssey settings, the video and audio signal, the channels
/// of the signal and of the speakers, and the HDMI output. See
/// [`appcommand0300_commands`] for those that a receiver gets.
const APPCOMMAND0300_COMMANDS: [Definition; 6] = [
    appcommand::GET_AUDYSSEY,
    appcommand::GET_VIDEO_INFO,
    appcommand::GET_AUDIO_INFO,
    appcommand::GET_INPUT_SIGNAL,
    appcommand::GET_ACTIVE_SPEAKER,
    appcommand::GET_OUTPUT_SETTINGS,
];

// `Answered0300` keeps one bit for each command in a `u8`.
const _: () = assert!(APPCOMMAND0300_COMMANDS.len() <= 8);

/// The commands of [`APPCOMMAND0300_COMMANDS`] whose item in an answer was
/// not an error item, one bit for each.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Answered0300(u8);

impl Answered0300 {
    /// The bit of `definition`. `0` for a command that is not in the list.
    fn bit(definition: &Definition) -> u8 {
        APPCOMMAND0300_COMMANDS
            .iter()
            .position(|known| known == definition)
            .map_or(0, |index| 1 << index)
    }

    fn add(&mut self, definition: &Definition) {
        self.0 |= Self::bit(definition);
    }

    fn has(self, definition: &Definition) -> bool {
        let bit = Self::bit(definition);
        bit != 0 && self.0 & bit == bit
    }

    /// The answer had the channels of the signal and of the speakers.
    fn has_channels(self) -> bool {
        self.has(&appcommand::GET_INPUT_SIGNAL) && self.has(&appcommand::GET_ACTIVE_SPEAKER)
    }

    /// Number of commands in the set.
    const fn count(self) -> u32 {
        self.0.count_ones()
    }
}

/// The body of the settings request for `part`, with its number of commands.
/// `None` if there is nothing to ask.
fn settings_request(info: &Info, part: SettingsPart) -> Option<(Bytes, usize)> {
    // `Missing::appcommand0300` is not checked: it is also set when none of
    // `APPCOMMAND0300_COMMANDS` is listed. A receiver that lists a settings
    // query has the file.
    let listed = info.queries?;
    if info.update_method != UpdateMethod::AppCommand {
        return None;
    }
    let mut body = Vec::new();
    match appcommand::write_settings_request(
        &listed,
        info.read_zone_count(),
        info.features,
        part,
        &mut body,
    ) {
        Ok(count @ 1..) => Some((body.into(), count)),
        _ => None,
    }
}

/// Sends a settings request of `count` commands and puts the answer into the
/// state. Telnet can open while the request runs. Then it asks for its
/// `groups` itself, and its answers are newer: values of these groups are left
/// out, and `forget` is called after the answer was read.
async fn read_settings_request(
    http: &HttpClient,
    channels: &Channels,
    (body, count): (Bytes, usize),
    brand: Brand,
    groups: &[QueryGroup],
    telnet_open: impl FnOnce() -> bool,
    forget: impl FnOnce(),
) -> Result<(), Error> {
    let mut opened = false;
    http.post_parsed(Endpoint::AppCommand0300.path(), body, |body| {
        opened = telnet_open();
        // A busy receiver can leave items out.
        let response = Response::parse_at_most(body, count)?;
        channels.modify(|state, notify| {
            appcommand::settings_lines(&response, |line, query| {
                if opened
                    && query
                        .group(brand)
                        .is_some_and(|group| groups.contains(&group))
                {
                    return;
                }
                if let Some(message) = Message::parse(line) {
                    notify(state.apply_message(&message));
                }
            });
        });
        Ok(())
    })
    .await?;
    if opened {
        forget();
    }
    Ok(())
}

/// The queries of [`APPCOMMAND0300_COMMANDS`] that the receiver answers. A
/// receiver can fail the whole request for a query that it does not know.
/// If `Deviceinfo.xml` lists the queries, only those are sent. If not, all
/// but the HDMI output are sent, which only newer receivers have.
fn appcommand0300_commands(queries: Option<&ListedQueries>) -> Box<[Definition]> {
    APPCOMMAND0300_COMMANDS
        .into_iter()
        .filter(|definition| asks_appcommand0300(queries, definition))
        .collect()
}

/// The query of [`APPCOMMAND0300_COMMANDS`] whose answer gives the value of
/// `query`. `None` if no query of `AppCommand0300.xml` gives it.
const fn appcommand0300_definition(query: Query) -> Option<Definition> {
    use Query as Q;
    match query {
        Q::MultEq | Q::DynamicEq | Q::ReferenceLevel | Q::DynamicVolume => {
            Some(appcommand::GET_AUDYSSEY)
        }
        Q::HdmiOutput => Some(appcommand::GET_OUTPUT_SETTINGS),
        Q::VideoResolution | Q::VideoInput | Q::VideoOutput => Some(appcommand::GET_VIDEO_INFO),
        Q::SamplingRate | Q::AudioSignal | Q::AudioSound => Some(appcommand::GET_AUDIO_INFO),
        Q::InputChannels => Some(appcommand::GET_INPUT_SIGNAL),
        Q::OutputChannels => Some(appcommand::GET_ACTIVE_SPEAKER),
        _ => None,
    }
}

/// Returns true if the receiver is asked `definition`, one of
/// [`APPCOMMAND0300_COMMANDS`].
fn asks_appcommand0300(queries: Option<&ListedQueries>, definition: &Definition) -> bool {
    queries.map_or_else(
        || *definition != appcommand::GET_OUTPUT_SETTINGS,
        |queries| queries.lists(definition.cmd.name.unwrap_or_default()),
    )
}

/// The queries of the first of the two `AppCommand0300.xml` requests of the
/// start: the audio signal and the channels. The start sends the others after
/// them, so that these come soon.
const APPCOMMAND0300_FIRST: [Definition; 3] = [
    appcommand::GET_AUDIO_INFO,
    appcommand::GET_INPUT_SIGNAL,
    appcommand::GET_ACTIVE_SPEAKER,
];

/// One request to `AppCommand0300.xml`: its queries and its body.
#[derive(Debug)]
struct Request0300 {
    commands: Box<[Definition]>,
    body: Bytes,
}

impl Request0300 {
    fn new(commands: Box<[Definition]>) -> Result<Self, Error> {
        let body = definitions_body(&commands)?;
        Ok(Self { commands, body })
    }
}

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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
///
/// Store it to connect faster next time with [`AvrBuilder::known`]. The
/// `serde` feature adds `Serialize` and `Deserialize`. A newer version of the
/// library can have more fields, so a stored value may not deserialize then:
/// connect without [`AvrBuilder::known`] in that case.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Info {
    /// Host that was given. An IPv6 address is without brackets.
    /// Link-local IPv6 addresses with a zone are not supported.
    pub host: String,
    /// Kind of HTTP interface of the receiver.
    pub receiver_type: ReceiverType,
    /// HTTP port that is used for AppCommand and the status pages.
    pub port: u16,
    /// What the receiver type is based on.
    pub type_proof: TypeProof,
    /// How the state is read.
    pub update_method: UpdateMethod,
    /// Brand from the device description, else from `BrandCode` in the
    /// device information, else from [`AvrBuilder::brand`], else
    /// [`Brand::Denon`].
    pub brand: Brand,
    /// Manufacturer from the device description.
    pub manufacturer: Option<String>,
    /// Model from the device description.
    pub model_name: Option<String>,
    /// Serial number from the device description. `None` if it is blank.
    pub serial_number: Option<String>,
    /// Name that the user gave the receiver.
    pub friendly_name: String,
    /// Version of the communication API. See [`Info::has_heos`].
    pub comm_api_version: Option<String>,
    /// MAC address of the wired network, as 12 hexadecimal digits without
    /// separators.
    pub mac_address: Option<String>,
    /// The receiver supports Dirac Live.
    pub dirac_live: bool,
    /// What the device information says about a tuner.
    pub tuner: TunerInfo,
    /// Number of quick selects (smart selects on Marantz) that the device
    /// information lists. `0` if it says that there are none, `None` if it
    /// does not say. A receiver can accept more than it lists.
    pub quick_select_count: Option<u8>,
    /// The `AppCommand` queries that `Deviceinfo.xml` lists. `None` if it has
    /// no list, as on receivers older than about 2015: then the library asks
    /// as if all but the HDMI output were there, but does not read the
    /// settings of `AppCommand0300.xml`. These are also not read when the
    /// list does not have version 2 of `GetSurroundParameter`
    /// ([`ListedQueries::surround_parameter`]).
    pub queries: Option<ListedQueries>,
    /// Zones that [`Avr::update`] reads. The main zone and the others that the
    /// receiver has. If [`Info::zone_count`] is known, the telnet queries of
    /// a zone that is not here are not sent.
    pub zones: Vec<Zone>,
    /// Number of zones that the device information lists. `None` if it does
    /// not say: [`Info::zones`] has the main zone only then, and the telnet
    /// queries of all zones are sent.
    pub zone_count: Option<u8>,
    /// The features that the device information lists. The state queries of
    /// a feature that it does not list are not sent at the start, with
    /// [`Avr::refresh`] and with `Avr::telnet_refresh_group`. `None` for
    /// receivers older than about 2016: then all are sent. Queries that the
    /// [`Info::spec_generation`] does not have are left out too.
    pub features: Option<DeviceFeatures>,
    /// The value ranges that the device information lists. Setters refuse a
    /// value outside a listed range or between its steps. A volume is checked
    /// against the highest volume of the zone only.
    pub ranges: DeviceRanges,
}

impl Info {
    /// Returns true if the device information lists a DAB tuner band.
    #[must_use]
    pub const fn has_dab(&self) -> bool {
        matches!(self.tuner, TunerInfo::Present(bands) if bands.dab)
    }

    /// Returns true if the receiver has HEOS: [`Info::comm_api_version`] is
    /// 0301 or higher.
    #[must_use]
    pub fn has_heos(&self) -> bool {
        self.spec_generation() == SpecGeneration::Current
    }

    /// The protocol specs that the receiver follows, from
    /// [`Info::comm_api_version`] and [`Info::brand`]. A Marantz 0210
    /// receiver counts as Denon if nothing tells its brand, see
    /// [`Info::brand`].
    #[must_use]
    pub fn spec_generation(&self) -> SpecGeneration {
        SpecGeneration::of(self.comm_api_version.as_deref(), self.brand)
    }

    /// Returns true if a spec of the generation of the receiver has the
    /// feature, or if no spec is known.
    #[must_use]
    pub fn supports(&self, feature: SpecFeature) -> bool {
        self.spec_generation().supports(feature)
    }

    /// Returns true if the receiver may have the value: the spec generation
    /// has it, the device information lists its feature and the receiver has
    /// its zone. What the device information does not say counts as present.
    #[must_use]
    pub fn offers(&self, query: Query) -> bool {
        self.offered().offers(query)
    }

    /// The rule of [`Info::offers`], as a copy that tasks can keep.
    pub(crate) fn offered(&self) -> Offered {
        Offered {
            generation: self.spec_generation(),
            features: self.features,
            other_zones: [Zone::Zone2, Zone::Zone3].map(|zone| self.has_zone(zone)),
        }
    }

    /// Returns true if the receiver may accept the command: as
    /// [`Info::offers_line`] for its line, the receiver has the zone, and the
    /// value is inside [`Info::ranges`]. A network audio command also needs
    /// [`SpecFeature::NetworkAudio`], and a preset number at most
    /// [`SpecGeneration::network_presets`]. A quick select needs
    /// [`Info::has_quick_select`]. False for a command whose text is not
    /// valid.
    #[must_use]
    pub fn offers_command(&self, zone: Zone, command: &Command<'_>) -> bool {
        use NetworkAudioCommand as N;
        use ZoneSettingValue as Z;
        let allowed = match *command {
            Command::NetworkAudio(N::Preset(number) | N::StorePreset(number)) => self
                .spec_generation()
                .network_presets()
                .is_some_and(|presets| number <= presets),
            Command::NetworkAudio(_) => self.supports(SpecFeature::NetworkAudio),
            Command::SetZoneSetting(Z::QuickSelect(slot) | Z::StoreQuickSelect(slot)) => {
                self.has_quick_select(slot)
            }
            _ => true,
        };
        if !allowed || !self.has_zone(zone) {
            return false;
        }
        let mut line = String::with_capacity(32);
        command.write_line(self.brand, zone, &mut line).is_ok()
            && self.offers_line(&line)
            && self.ranges.check(zone, command).is_ok()
    }

    /// Returns true if the receiver may accept the command line, for example
    /// `Z2PSBAS 50`: the spec generation has what it needs, and the device
    /// information lists the feature and the zone of the value that it sets.
    /// Values are not checked against [`Info::ranges`].
    #[must_use]
    pub fn offers_line(&self, line: &str) -> bool {
        self.spec_generation().allows_line(line)
            && query_for_command(line).is_none_or(|query| self.lists(query))
    }

    /// Returns true if the device information lists the feature and the zone
    /// of the value, or does not say.
    fn lists(&self, query: Query) -> bool {
        self.offered().lists(query)
    }

    /// Returns true if the device information lists the zone, or does not
    /// say how many zones there are.
    fn has_zone(&self, zone: Zone) -> bool {
        zone == Zone::Main || self.zone_count.is_none() || self.zones.contains(&zone)
    }

    /// Returns true if the receiver has neither zone 2 nor zone 3, see
    /// [`Info::has_zone`].
    pub(crate) fn single_zone(&self) -> bool {
        !self.has_zone(Zone::Zone2) && !self.has_zone(Zone::Zone3)
    }

    /// The number of zones that [`Avr::update`] reads.
    fn read_zone_count(&self) -> u8 {
        u8::try_from(self.zones.len()).unwrap_or(u8::MAX)
    }

    /// Returns true if the device information lists the quick select (or
    /// SMART select) with this number, counted from 1. The number is at most
    /// [`MAX_QUICK_SELECT`], and at most [`Info::quick_select_count`] if that
    /// is known. It follows the list, also when a receiver accepts more.
    #[must_use]
    pub const fn has_quick_select(&self, slot: u8) -> bool {
        let listed = match self.quick_select_count {
            Some(count) => slot <= count,
            None => true,
        };
        slot >= 1 && slot <= MAX_QUICK_SELECT && listed
    }

    /// Returns true if [`Info::queries`] lists `GetOutputSettings`: the
    /// receiver answers the HDMI output over HTTP, so
    /// [`Avr::update_signal_and_settings`] and the read after
    /// `set_hdmi_output` over HTTP read it.
    #[must_use]
    pub fn hdmi_output_over_http(&self) -> bool {
        asks_appcommand0300(self.queries.as_ref(), &appcommand::GET_OUTPUT_SETTINGS)
    }

    /// Returns true if an HTTP read gives the value, so that it is known
    /// without telnet. Same as `!self.http_reads(query).is_empty()`, see
    /// [`Info::http_reads`].
    #[must_use]
    pub fn readable_over_http(&self, query: Query) -> bool {
        !self.http_reads(query).is_empty()
    }

    /// The methods of [`Avr`] that read the value over HTTP. Empty if no HTTP
    /// read gives it, so that only telnet can tell it. Only zones in
    /// [`Info::zones`] are read. The set tells what the receiver can give
    /// from its device information. [`Avr::http_reads`] also leaves out what
    /// the receiver does not have, see [`Avr::missing_sources`].
    ///
    /// [`HttpReads::REFRESH`] is [`Avr::refresh`] without telnet: it does what
    /// [`Avr::update`] does and reads the settings of `AppCommand0300.xml`.
    /// The start reads these settings too (if
    /// [`StartOptions::settings`](crate::StartOptions::settings) is on), and
    /// so do the reads after commands. The LFE level comes only when the input
    /// mode is not 7.1CH IN.
    ///
    /// Where the signal values come from:
    ///
    /// - The resolution, the audio signal and the channels:
    ///   `AppCommand0300.xml`, if the receiver answers it
    ///   ([`HttpReads::SIGNAL_AND_SETTINGS`]).
    /// - HDR, the color space, the pixel depth, the FRL rate and BT.2020: the
    ///   video page of the web interface. The resolution is on it too.
    /// - The channels, if `AppCommand0300.xml` does not give them: the
    ///   channels page of the web interface.
    ///
    /// [`Avr::refresh_signal_pages`] ([`HttpReads::SIGNAL_PAGES`]) reads
    /// `AppCommand0300.xml` first, then the video page, and the channels page
    /// when `AppCommand0300.xml` does not give the channels, in this answer or
    /// at the last start. A page that
    /// answered 404 is not asked again.
    /// The start reads the two pages only if
    /// [`StartOptions::signal_pages`](crate::StartOptions::signal_pages) is
    /// on. A value stays unknown if the receiver has no such page: the start
    /// then gives [`PageStatus::NotFound`] for it. The video page has BT.2020
    /// only for some signals.
    ///
    /// Status pages of some receivers do not tell which sources are in use.
    /// On receivers without `AppCommand`, the friendly name is only in
    /// [`Info::friendly_name`].
    #[must_use]
    pub fn http_reads(&self, query: Query) -> HttpReads {
        self.http_reads_without(query, MissingSources::default())
    }

    /// [`Info::http_reads`] without the sources in `missing`. With
    /// [`Avr::missing_sources`], this gives the same as [`Avr::http_reads`].
    #[must_use]
    pub fn http_reads_without(&self, query: Query, missing: MissingSources) -> HttpReads {
        use Query as Q;
        if !self.offers(query) || !self.zones.contains(&query.zone()) {
            return HttpReads::EMPTY;
        }
        // The settings rows use the other query of the same value.
        if query == Q::AutoLipSyncAls {
            return self.http_reads_without(Q::AutoLipSync, missing);
        }
        let appcommand = self.update_method == UpdateMethod::AppCommand;
        let queries = self.queries.as_ref();
        let asks = |definition| appcommand && asks_appcommand0300(queries, &definition);
        let part = query.signal_part();
        let update = match query {
            Q::Source
            | Q::Volume
            | Q::Mute
            | Q::Zone2Power
            | Q::Zone2Mute
            | Q::Zone3Power
            | Q::Zone3Mute
            | Q::SoundMode
            | Q::SourceNames
            | Q::SourcesInUse
            | Q::Eco => true,
            Q::FriendlyName
            | Q::Dimmer
            | Q::AutoStandby
            | Q::Zone2AutoStandby
            | Q::Zone3AutoStandby => appcommand,
            Q::ToneControl | Q::Bass | Q::Treble => {
                appcommand
                    && !missing.tone_control
                    && queries.is_none_or(|queries| queries.tone_control)
            }
            _ => false,
        };
        let appcommand0300 =
            !missing.appcommand0300 && appcommand0300_definition(query).is_some_and(asks);
        // The pages are read with any update method.
        let page = match part {
            Some(SignalPart::Video | SignalPart::Colorimetry) => !missing.video_page,
            Some(SignalPart::Channels) => !missing.channels_page,
            _ => false,
        };
        let settings = appcommand
            && queries.is_some_and(|queries| {
                settings_request_asks(query, queries, self.read_zone_count(), self.features)
            });
        let mut reads = HttpReads::EMPTY;
        if update {
            reads |= HttpReads::UPDATE | HttpReads::REFRESH;
        }
        if appcommand0300 {
            reads |= HttpReads::SIGNAL_AND_SETTINGS | HttpReads::SIGNAL_PAGES;
        }
        if page {
            reads |= HttpReads::SIGNAL_PAGES;
        }
        if settings {
            reads |= HttpReads::REFRESH;
        }
        reads
    }
}

/// HTTP sources that the receiver does not have, see
/// [`Avr::missing_sources`].
///
/// Store it with [`Info`] and give it to [`AvrBuilder::missing`], so that a
/// new [`Avr`] does not ask these sources again. The `serde` feature adds
/// `Serialize` and `Deserialize`. The field names are the stored names. A
/// field that is not in the stored value is false, and an unknown field is
/// ignored.
#[cfg_attr(
    feature = "serde",
    derive(serde::Serialize, serde::Deserialize),
    serde(default)
)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[expect(clippy::struct_excessive_bools, reason = "one flag for each source")]
pub struct MissingSources {
    /// `AppCommand0300.xml` answered 404, or `Deviceinfo.xml` lists none of
    /// its queries. The values that only this file gives are left out. The
    /// settings request also goes to this file, but it is still sent.
    pub appcommand0300: bool,
    /// Tone control is left out of the reads: `Deviceinfo.xml` does not list
    /// it, or a request with it failed and a request without it was
    /// answered.
    pub tone_control: bool,
    /// The video signal page of the web interface answered 404.
    pub video_page: bool,
    /// The channels page of the web interface answered 404.
    pub channels_page: bool,
}

/// A set of [`Avr`] methods that read values over HTTP, see
/// [`Info::http_reads`]. Combine them with `|`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct HttpReads(u8);

impl HttpReads {
    /// No method.
    pub const EMPTY: Self = Self(0);
    /// [`Avr::update`].
    pub const UPDATE: Self = Self(1);
    /// [`Avr::refresh`] while telnet is not connected. It reads all that
    /// [`HttpReads::UPDATE`] reads, and the settings of `AppCommand0300.xml`.
    pub const REFRESH: Self = Self(1 << 1);
    /// [`Avr::update_signal_and_settings`].
    pub const SIGNAL_AND_SETTINGS: Self = Self(1 << 2);
    /// [`Avr::refresh_signal_pages`]. It reads all that
    /// [`HttpReads::SIGNAL_AND_SETTINGS`] reads, and the signal pages.
    pub const SIGNAL_PAGES: Self = Self(1 << 3);

    /// Names for `Debug`, in bit order.
    const NAMES: [(Self, &str); 4] = [
        (Self::UPDATE, "UPDATE"),
        (Self::REFRESH, "REFRESH"),
        (Self::SIGNAL_AND_SETTINGS, "SIGNAL_AND_SETTINGS"),
        (Self::SIGNAL_PAGES, "SIGNAL_PAGES"),
    ];

    /// Returns true if the set has no method.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// Returns true if the set has all methods of `other`.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Returns true if the set has at least one method of `other`.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// The methods that are in one of the sets or in both.
    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl std::ops::BitOr for HttpReads {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(other)
    }
}

impl std::ops::BitOrAssign for HttpReads {
    fn bitor_assign(&mut self, other: Self) {
        *self = self.union(other);
    }
}

impl std::fmt::Debug for HttpReads {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HttpReads(")?;
        let mut first = true;
        for (bit, name) in Self::NAMES {
            if self.contains(bit) {
                if !first {
                    f.write_str(" | ")?;
                }
                f.write_str(name)?;
                first = false;
            }
        }
        f.write_str(")")
    }
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
    connect_retry: Duration::ZERO,
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
    brand: Option<Brand>,
    known: Option<Info>,
    missing: MissingSources,
    follow_up_read: Option<FollowUpRead>,
    background_pause: Duration,
    #[cfg(feature = "telnet")]
    telnet: TelnetConfig,
    #[cfg(feature = "heos")]
    heos: denon_marantz_heos::HeosConfig,
}

/// A receiver that is read over HTTP.
///
/// The state is in a [`watch`] channel. [`Avr::start`] or [`Avr::update`]
/// reads it from the receiver. Everything is `&self`, so one value can be
/// shared between tasks. See the crate docs for the meaning of the prefixes
/// `update_`, `telnet_` and `refresh`.
#[derive(Debug)]
pub struct Avr {
    http: HttpClient,
    /// The web interface on its own port.
    web_interface: HttpClient,
    info: Info,
    /// The request body of [`UPDATE_COMMANDS`].
    update_body: Bytes,
    /// The request body of [`UPDATE_COMMANDS_WITHOUT_TONE`].
    update_body_without_tone: Bytes,
    /// The request body of [`HTTP_ONLY_COMMANDS`].
    #[cfg(feature = "telnet")]
    http_only_body: Bytes,
    /// The request body of [`HTTP_ONLY_WITH_DEVICE_COMMANDS`].
    #[cfg(feature = "telnet")]
    http_only_with_device_body: Bytes,
    /// The queries of [`Avr::update_signal_and_settings`], see
    /// [`appcommand0300_commands`].
    appcommand0300_commands: Box<[Definition]>,
    /// The request body of `appcommand0300_commands`.
    appcommand0300_body: Bytes,
    /// `appcommand0300_commands` in the two requests of the start, see
    /// [`APPCOMMAND0300_FIRST`].
    appcommand0300_parts: [Request0300; 2],
    /// Reads that the receiver does not answer.
    missing: Arc<Missing>,
    /// The commands of the first `AppCommand0300.xml` request of the last
    /// start (see [`APPCOMMAND0300_FIRST`]) that were answered without an
    /// error item, as [`Answered0300`].
    signal_answered: AtomicU8,
    channels: Channels,
    /// A start is running, see [`Avr::start_with`].
    starting: AtomicBool,
    #[cfg(feature = "telnet")]
    telnet: Telnet,
    #[cfg(feature = "heos")]
    heos: HeosOwner,
    /// The reset key that was given out last.
    reset_keys: ResetKeys,
    /// Commands over HTTP go one after the other, and before the HTTP reads.
    priority: Arc<Priority>,
    /// Reads the values that a command over HTTP changed.
    follow_up: Option<FollowUp>,
    /// The info came from [`AvrBuilder::known`], so the start checks it.
    known: bool,
}

/// Reads that the receiver does not answer, so they are not asked again.
#[derive(Debug)]
struct Missing {
    /// `AppCommand0300.xml` answered 404, none of
    /// [`APPCOMMAND0300_COMMANDS`] is listed, or the stored value says so.
    appcommand0300: AtomicBool,
    /// `Deviceinfo.xml` does not list tone control, a request with it failed
    /// and a request without it was answered, or the stored value says so.
    tone_control: AtomicBool,
    /// The receiver is older than the 2016 models, which all have tone
    /// control, and `Deviceinfo.xml` does not say if it has it.
    tone_control_optional: bool,
    /// The video signal page of the web interface answered 404.
    video_page: AtomicBool,
    /// The channels page of the web interface answered 404.
    channels_page: AtomicBool,
}

impl Missing {
    /// `queries` is what `Deviceinfo.xml` lists. If it lists nothing, tone
    /// control is asked, and on `AvrX` receivers a failure can drop it, if a
    /// request without tone control is answered.
    /// `no_appcommand0300` is true if no query of that file is left to send.
    /// `stored` is what an earlier connection found missing.
    fn new(
        receiver_type: ReceiverType,
        queries: Option<&ListedQueries>,
        no_appcommand0300: bool,
        stored: MissingSources,
    ) -> Self {
        Self {
            appcommand0300: AtomicBool::new(no_appcommand0300 || stored.appcommand0300),
            tone_control: AtomicBool::new(
                queries.is_some_and(|queries| !queries.tone_control) || stored.tone_control,
            ),
            tone_control_optional: queries.is_none() && receiver_type == ReceiverType::AvrX,
            video_page: AtomicBool::new(stored.video_page),
            channels_page: AtomicBool::new(stored.channels_page),
        }
    }

    /// The sources that are missing now.
    fn sources(&self) -> MissingSources {
        MissingSources {
            appcommand0300: self.appcommand0300.load(Ordering::Relaxed),
            tone_control: self.tone_control.load(Ordering::Relaxed),
            video_page: self.video_page.load(Ordering::Relaxed),
            channels_page: self.channels_page.load(Ordering::Relaxed),
        }
    }

    /// A request with tone control that failed with `error` can mean that
    /// the receiver does not have tone control. It may leave out the item or
    /// refuse the request.
    const fn may_lack_tone_control(&self, error: &Error) -> bool {
        self.tone_control_optional && !error.is_unreachable()
    }
}

impl Avr {
    /// Starts the settings for a receiver at `host`: a host name or an IP
    /// address. An IPv6 address can have brackets. Link-local IPv6 addresses
    /// with a zone, for example `fe80::1%en0`, are not supported.
    #[must_use]
    pub fn builder(host: impl Into<String>) -> AvrBuilder {
        AvrBuilder {
            host: denon_marantz_shared::host::into_bare(host.into()),
            http: HttpConfig::default(),
            web_interface: WEB_INTERFACE_CONFIG,
            web_interface_port: WEB_INTERFACE_PORT,
            probes: None,
            description_port: None,
            brand: None,
            known: None,
            missing: MissingSources::default(),
            follow_up_read: Some(FollowUpRead::default()),
            background_pause: Duration::from_millis(250),
            #[cfg(feature = "telnet")]
            telnet: TelnetConfig::default(),
            #[cfg(feature = "heos")]
            heos: denon_marantz_heos::HeosConfig::default(),
        }
    }

    /// What is known about the receiver.
    #[must_use]
    pub const fn info(&self) -> &Info {
        &self.info
    }

    /// Like [`Info::http_reads`], without the sources that the receiver
    /// does not have, see [`Avr::missing_sources`].
    #[must_use]
    pub fn http_reads(&self, query: Query) -> HttpReads {
        self.info.http_reads_without(query, self.missing.sources())
    }

    /// The HTTP sources that the receiver does not have:
    /// `AppCommand0300.xml` or one of the two signal pages after it answered
    /// 404, and tone control after a read dropped it. These sources are not
    /// asked again while this [`Avr`] lives.
    ///
    /// Before a read tried a source, it counts as there, unless
    /// `Deviceinfo.xml` or [`AvrBuilder::missing`] gave it. The start tries
    /// most of them, so call this after it, for example after
    /// [`Started::basic_synced`](crate::Started::basic_synced). Later reads
    /// can add more: [`Avr::update`], the reads after commands and
    /// [`Avr::refresh_signal_pages`]. A source is never removed.
    #[must_use]
    pub fn missing_sources(&self) -> MissingSources {
        self.missing.sources()
    }

    /// Reads the device information and the update method again, and
    /// compares them with [`Avr::info`]. Returns the new values if they
    /// differ. Then connect again with them, see [`AvrBuilder::known`]. The
    /// brand, the manufacturer, the model and the serial number are kept,
    /// because the device description is not read again. A friendly name
    /// that the receiver does not give is kept too.
    ///
    /// It asks only the type and the port of [`Avr::info`]. A receiver that
    /// moved to another port gives an error then: connect again without
    /// [`AvrBuilder::known`].
    ///
    /// # Errors
    ///
    /// Returns an error if the receiver cannot be reached, and
    /// [`Error::NothingRead`] if [`Avr::info`] has a proven type but the
    /// device information is not readable now. That can be a short failure,
    /// so it is not reported as a change.
    pub async fn verify(&self) -> Result<Option<Info>, Error> {
        self.in_span(self.verify_in()).await
    }

    /// Does the work of [`Avr::verify`].
    async fn verify_in(&self) -> Result<Option<Info>, Error> {
        let stored = &self.info;
        let config = self.http.config();
        let identified =
            identify_on(&stored.host, &[(stored.receiver_type, stored.port)], config).await?;
        if stored.type_proof.is_proven() && !identified.type_proof.is_proven() {
            return Err(Error::NothingRead);
        }
        let other_port;
        let http = if identified.port == stored.port {
            &self.http
        } else {
            other_port = HttpClient::new(&stored.host, identified.port, config)?;
            &other_port
        };
        let (update_method, friendly_name) =
            detect_update_method(http, identified.receiver_type).await?;
        let mut info = info_from(
            stored.host.clone(),
            identified,
            update_method,
            stored.brand,
            friendly_name.unwrap_or_else(|| stored.friendly_name.clone()),
            None,
        );
        info.manufacturer.clone_from(&stored.manufacturer);
        info.model_name.clone_from(&stored.model_name);
        info.serial_number.clone_from(&stored.serial_number);
        Ok((info != *stored).then_some(info))
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
    /// An `AvrX` whose answer fails with tone control and works without it is
    /// read without tone control from then on.
    pub async fn update(&self) -> Result<(), Error> {
        self.in_span(self.update_in()).await
    }

    /// Does the work of [`Avr::update`].
    async fn update_in(&self) -> Result<(), Error> {
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
    /// does, and then reads the settings over `AppCommand0300.xml` that the
    /// start reads without telnet.
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
    /// `Avr::telnet_refresh` and of the read of the zone names. If
    /// [`Avr::update`] fails, the settings are not read. If the fast part of
    /// the settings fails, the slow part is not read. Of the settings, only
    /// a timeout or a network error is returned; other errors are only
    /// logged.
    pub async fn refresh(&self) -> Result<(), Error> {
        self.in_span(self.refresh_in()).await
    }

    /// Does the work of [`Avr::refresh`].
    async fn refresh_in(&self) -> Result<(), Error> {
        #[cfg(feature = "telnet")]
        if self.telnet_connected() {
            let http = self.update_http_only().await;
            return http.and(self.telnet_refresh().await);
        }
        self.update().await?;
        // Only a receiver that does not answer counts as an error: a refused
        // settings query must not make each refresh fail.
        let settings = match self.read_settings(SettingsPart::Fast).await {
            Ok(_) => self.read_settings(SettingsPart::Slow).await,
            error => error,
        };
        match settings {
            Err(error) if error.is_unreachable() => Err(error),
            _ => Ok(()),
        }
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
        self.in_span(self.update_speaker_details_in()).await
    }

    /// Does the work of [`Avr::update_speaker_details`].
    async fn update_speaker_details_in(&self) -> Result<SpeakerDetails, Error> {
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
        self.in_span(self.update_source_details_in()).await
    }

    /// Does the work of [`Avr::update_source_details`].
    async fn update_source_details_in(&self) -> Result<SourceDetails, Error> {
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
        self.in_span(self.update_settings_availability_in()).await
    }

    /// Does the work of [`Avr::update_settings_availability`].
    async fn update_settings_availability_in(&self) -> Result<SettingsAvailabilityDetails, Error> {
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
        self.in_span(self.update_detailed_availability_in()).await
    }

    /// Does the work of [`Avr::update_detailed_availability`].
    async fn update_detailed_availability_in(&self) -> Result<SettingsAvailabilityDetails, Error> {
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
        self.in_span(self.update_video_details_in()).await
    }

    /// Does the work of [`Avr::update_video_details`].
    async fn update_video_details_in(&self) -> Result<VideoMenuDetails, Error> {
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
        self.in_span(self.update_general_settings_in()).await
    }

    /// Does the work of [`Avr::update_general_settings`].
    async fn update_general_settings_in(&self) -> Result<GeneralDetails, Error> {
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
        self.in_span(self.update_system_settings_in()).await
    }

    /// Does the work of [`Avr::update_system_settings`].
    async fn update_system_settings_in(&self) -> Result<PageStatus, Error> {
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
        self.in_span(self.update_now_playing_in()).await
    }

    /// Does the work of [`Avr::update_now_playing`].
    async fn update_now_playing_in(&self) -> Result<PageStatus, Error> {
        let result = self.read_now_playing_page().await;
        self.forget_telnet_values(&result);
        result
    }

    async fn read_now_playing_page(&self) -> Result<PageStatus, Error> {
        match self.channels.read(State::main_playing_kind) {
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
        self.in_span(self.refresh_now_playing_in()).await
    }

    /// Does the work of [`Avr::refresh_now_playing`].
    async fn refresh_now_playing_in(&self) -> Result<(), Error> {
        #[cfg(feature = "telnet")]
        if self.telnet_connected() {
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

        let host = denon_marantz_shared::host::UrlHost(&self.info.host);
        let port = self.info.port;
        match self.channels.read(State::main_playing_kind)? {
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

    /// `work`, which runs in the span of this `Avr` when it is awaited.
    fn in_span<F: Future>(&self, work: F) -> tracing::instrument::Instrumented<F> {
        work.instrument(self.http.span().clone())
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

    /// Opens the HEOS connection and keeps it open, for the playback of the
    /// HEOS player of the receiver: see [`Avr::heos`]. Volume and mute stay on
    /// telnet and HTTP. If the connection is lost, a task connects again.
    /// Does nothing while the connection is open. Waits for a connect that
    /// runs, also the one of the start. Dropping the `Avr` closes it. After a
    /// shutdown through a kept [`HeosPlayer::client`], it opens a new one.
    ///
    /// # Errors
    ///
    /// [`Error::HeosNotSupported`] if [`Info::has_heos`] is false. If the
    /// first connection fails, [`Error::Heos`], or [`Error::Network`] or
    /// [`Error::Timeout`] if the host name could not be resolved, and the
    /// status is [`HeosPlayerStatus::Failed`]. Nothing keeps trying then. The
    /// resolve and the connect each get
    /// [`HeosConfig::connect_timeout`](denon_marantz_heos::HeosConfig::connect_timeout).
    #[cfg(feature = "heos")]
    pub async fn heos_connect(&self) -> Result<(), Error> {
        self.heos.connect().await.map(|_| ())
    }

    /// The error of the HEOS connect that the start ran in the background,
    /// if it failed, see [`StartOptions::heos`](crate::StartOptions::heos).
    /// The status is [`HeosPlayerStatus::Failed`] then. It is cleared when a
    /// new connect begins and by [`Avr::heos_disconnect`]. A failed
    /// [`Avr::heos_connect`] returns its error instead.
    #[cfg(feature = "heos")]
    #[must_use]
    pub fn heos_start_error(&self) -> Option<Arc<Error>> {
        self.heos.start_error()
    }

    /// Closes the HEOS connection, also for a client from
    /// [`HeosPlayer::client`] that is kept. Stops a connect that the start
    /// runs in the background.
    #[cfg(feature = "heos")]
    pub async fn heos_disconnect(&self) {
        self.heos.disconnect().await;
    }

    /// Watches the HEOS player of the receiver. It is
    /// [`HeosPlayerStatus::Ready`] when [`Avr::heos`] gives the player.
    #[cfg(feature = "heos")]
    #[must_use]
    pub fn watch_heos_status(&self) -> watch::Receiver<HeosPlayerStatus> {
        self.heos.watch_status()
    }

    /// Subscribes to the changes of the HEOS players, as
    /// [`Heos::changes`](denon_marantz_heos::Heos::changes) gives them. One
    /// subscription gets the changes of each connection, also after
    /// [`Avr::heos_disconnect`] and a new [`Avr::heos_connect`]. Read the
    /// values with [`Avr::heos`]. The changes of the first read of a
    /// connection are not passed on: read the state when
    /// [`Avr::watch_heos_status`] becomes [`HeosPlayerStatus::Ready`]. A
    /// subscriber that falls behind loses the oldest changes and gets
    /// `Lagged`: read the state again then. If the library itself loses
    /// changes of the connection, it sends a change of each kind for each
    /// player instead.
    #[cfg(feature = "heos")]
    #[must_use]
    pub fn heos_changes(&self) -> broadcast::Receiver<denon_marantz_heos::Change> {
        self.heos.changes()
    }

    /// The HEOS player of the receiver, while [`Avr::watch_heos_status`] is
    /// [`HeosPlayerStatus::Ready`]. It is the player with the address of the
    /// receiver, else the one with its serial number
    /// ([`Info::serial_number`]). If neither matches, for example when a host
    /// name resolves to an IPv6 address and HEOS lists an IPv4 address, and
    /// the serial number is not known, the status stays
    /// [`HeosPlayerStatus::WaitingForPlayer`].
    #[cfg(feature = "heos")]
    #[must_use]
    pub fn heos(&self) -> Option<HeosPlayer> {
        self.heos.player()
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
    /// fill the state, see [`AvrBuilder::background_pause`]. This returns when
    /// the command is written. That does not mean that the receiver did it:
    /// its answer is an event. If the command is waiting when the future is
    /// dropped, it is still sent.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotConnected`] if the telnet connection is not open,
    /// or if it ends while the command waits. The command is then dropped, so
    /// it is never sent late. Returns [`Error::QueueFull`] if too many lines wait
    /// to be sent. Returns [`Error::InvalidCommand`] if the command has text
    /// that cannot be sent, and [`Error::NumberOutOfRange`] for a value
    /// outside a range in [`Info::ranges`] or between its steps.
    ///
    /// [`Error::NotConnected`] always means that the command was not written,
    /// so it can be sent again. A command that may have been written, at
    /// least in part, gives [`Error::Network`] with `sent` set to true: the
    /// receiver may have got the line. In some rare cases, for example when
    /// the connection task stops, a command that was not written gives this
    /// error too.
    ///
    /// After a power-on command while the state shows no zone on, the next
    /// line waits, see [`TelnetConfig::power_on_hold`].
    #[cfg(feature = "telnet")]
    pub async fn telnet_send(&self, zone: Zone, command: Command<'_>) -> Result<(), Error> {
        let line = command::encode_checked(self.info.brand, &self.info.ranges, zone, &command)?;
        let hold = matches!(command, Command::DevicePowerOn | Command::PowerOn)
            && self.channels.read(|state| {
                Zone::ALL
                    .iter()
                    .all(|&each| state.zone(each).power != Some(denon_marantz_proto::Power::On))
            });
        self.telnet.send(line, hold).await
    }

    /// Asks the receiver on telnet what it plays. The answer comes as events
    /// that change [`DeviceState::now_playing`](denon_marantz_proto::DeviceState::now_playing)
    /// or the tuner state.
    ///
    /// It asks for the lines of network audio, the HD radio information or the
    /// tuner frequency and name (and the DAB status on DAB models, see
    /// [`Info::has_dab`]), for the source of the main zone, and does
    /// nothing for another source. Ask again to see changes. A receiver that
    /// uses HEOS for network audio does not answer.
    ///
    /// # Errors
    ///
    /// The same as [`Avr::telnet_send`].
    #[cfg(feature = "telnet")]
    pub async fn telnet_refresh_now_playing(&self) -> Result<(), Error> {
        match self.channels.read(State::main_playing_kind) {
            Some(PlayingKind::NetAudio) => self.telnet_send(Zone::Main, Command::Raw("NSE")).await,
            Some(PlayingKind::HdRadio) => {
                let send = async |command| self.telnet_send(Zone::Main, command).await;
                send_queries(&denon_marantz_proto::HD_RADIO_QUERIES, send).await
            }
            Some(PlayingKind::Tuner) => {
                let send = async |command| self.telnet_send(Zone::Main, command).await;
                send_queries(&denon_marantz_proto::TUNER_QUERIES, send).await?;
                if self.info.has_dab() {
                    send_queries(&denon_marantz_proto::DAB_QUERIES, send).await?;
                }
                let query = denon_marantz_proto::station_name_query(self.info.brand);
                self.telnet_send(Zone::Main, Command::Raw(query)).await
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
        if self.telnet_connected() {
            return self.telnet_send(zone, command).await;
        }
        self.http_send(zone, command).await
    }

    /// Sends a command over HTTP, with the page `formiPhoneAppDirect.xml`
    /// of the receiver.
    ///
    /// Commands wait for each other, in the order of the calls, and for the
    /// spacing of the HTTP requests (see [`AvrBuilder::http_config`]). They go
    /// before the HTTP reads that wait, also those of the web interface, see
    /// [`AvrBuilder::background_pause`]. A read that has started is not
    /// stopped. This returns when the receiver has answered the request. The
    /// answer does not say if the receiver did the command, as on telnet. The
    /// receiver does not send events over HTTP, so a command that changes a
    /// value of [`Avr::update`] makes a read of these values later, see
    /// [`AvrBuilder::follow_up_read`]. So does a command for one of the
    /// settings that the start reads over `AppCommand0300.xml`. A sound mode
    /// command reads the fast ones of these settings again, and a source
    /// command or a quick select of the main zone all of them. The settings
    /// reads are skipped while telnet is open. The values of other settings
    /// change when they are read.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`] if the command has text that cannot be
    /// sent, and [`Error::NumberOutOfRange`] for a value outside a range in
    /// [`Info::ranges`] or between its steps. Returns
    /// [`Error::QueryNeedsTelnet`] for a query (a command that asks for a
    /// value): the receiver answers it on telnet only. Returns the errors of
    /// an HTTP request if it fails.
    pub async fn http_send(&self, zone: Zone, command: Command<'_>) -> Result<(), Error> {
        let line = command::encode_checked(self.info.brand, &self.info.ranges, zone, &command)?;
        if command::is_query(&line) {
            return Err(Error::QueryNeedsTelnet);
        }
        {
            let _turn = self.priority.command().await;
            self.http.command(&command::http_path(&line)).await?;
        }
        if let Some(follow_up) = &self.follow_up {
            if let Some(reads) = Reads::after(zone, &command) {
                follow_up.note(reads);
            }
            for part in appcommand::settings_parts_of_command(&line) {
                follow_up.note(if *part == SettingsPart::Fast {
                    Reads::Settings
                } else {
                    Reads::SlowSettings
                });
            }
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

    /// Waits until the telnet queries are answered: no query waits to be
    /// written, each written query has its answer, and each list has its
    /// `END` line. A query without an answer for
    /// [`TelnetConfig::answer_quiet`] counts as not answered: some receivers
    /// never answer some queries. Gives the queries that were not answered,
    /// each once. A query whose answer comes late, but before this returns,
    /// is not given.
    /// Queries that are added while this waits are covered too. Queries that
    /// the connection asks by itself, for example after a signal hint or for
    /// a signal check, are not given; their answers still change the state
    /// and send events. See also [`Started::all_synced`].
    ///
    /// The receiver sends each answer to every telnet connection, so an
    /// answer to a query of another client also counts. Its value is just as
    /// new. If no query waits, this returns after a short time without a
    /// write, so it does not show that the connection works. Dropping the
    /// future is safe.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NotConnected`] if the telnet connection is not open or
    /// ends first, and an error for which [`Error::is_timeout`] is true if
    /// this takes longer than the keep-alive time.
    #[cfg(feature = "telnet")]
    pub async fn telnet_sync(&self) -> Result<Vec<std::borrow::Cow<'static, str>>, Error> {
        self.telnet.sync().await
    }

    /// As [`Avr::telnet_sync`], for the start, with this limit. Gives the
    /// open queries if the limit comes first. The queries that were not
    /// answered before this call count too.
    #[cfg(feature = "telnet")]
    pub(crate) async fn telnet_sync_start(
        &self,
        limit: Duration,
    ) -> Result<crate::telnet::SyncResult, Error> {
        self.telnet.sync_start(limit).await
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
    /// asks a query as text. Send only queries from the protocol
    /// specification: the receiver can read unknown text that ends with `?`
    /// as a setter and change a setting.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidRequest`] if a value has no query for the brand
    /// of the receiver, or if a raw query does not end with `?`. Returns
    /// [`Error::InvalidCommand`] if the text of a raw query is empty or has a
    /// control character. Returns [`Error::NotConnected`] if the telnet
    /// connection is not open. Nothing is asked if there is an error. The
    /// queue limit of [`Avr::telnet_send`] does not apply here.
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
    /// is called. Queries of features that [`Info::features`] does not list,
    /// that [`Info::spec_generation`] does not have, or of a zone that the
    /// receiver does not have (see [`Info::zone_count`]), are not sent. A
    /// group can then be empty.
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
        if !self.missing.tone_control.load(Ordering::Relaxed) {
            let result = self
                .read_appcommand(
                    Endpoint::AppCommand,
                    &UPDATE_COMMANDS,
                    self.update_body.clone(),
                )
                .await;
            let Err(error) = &result else {
                return result;
            };
            if !self.missing.may_lack_tone_control(error) {
                return result;
            }
            let without = self.update_without_tone().await;
            if without.is_ok() {
                tracing::debug!(host = %self.info.host, "tone control is left out of the update");
                self.missing.tone_control.store(true, Ordering::Relaxed);
                return without;
            }
            return result;
        }
        self.update_without_tone().await
    }

    async fn update_without_tone(&self) -> Result<(), Error> {
        self.read_appcommand(
            Endpoint::AppCommand,
            UPDATE_COMMANDS_WITHOUT_TONE,
            self.update_body_without_tone.clone(),
        )
        .await
    }

    /// Reads the video and audio signal, the channels, the Audyssey settings
    /// and the HDMI output from the receiver with one request to
    /// `AppCommand0300.xml`. The HDMI output is read only if
    /// [`Info::hdmi_output_over_http`] is true.
    ///
    /// This is faster than the pages of the web interface and the telnet
    /// queries for the same values. It gives the
    /// resolution that the receiver gets and sends, the kind of the audio
    /// signal and of the sound, the sampling rate, and the channels of the
    /// signal and of the speakers. HDR, the pixel depth, the color space and
    /// the FRL rate are not in it: [`Avr::refresh_signal`] reads them over
    /// telnet, or from the web interface without telnet.
    /// [`Avr::refresh_signal_pages`] reads them from the web interface; it
    /// also does this read, so do not call both. The values are in
    /// [`DeviceState::signal`](denon_marantz_proto::DeviceState::signal) and,
    /// for Audyssey and the HDMI output, in the fields of
    /// [`DeviceState`](denon_marantz_proto::DeviceState).
    ///
    /// Receivers that are read with `AppCommand` are asked. If
    /// `Deviceinfo.xml` lists the queries, as on receivers from about 2015,
    /// only those are sent. The result is [`PageStatus::NotFound`] for
    /// another receiver, for a receiver that lists none of the queries and
    /// for a receiver that answers 404, and then it is not asked again.
    /// Another refusal or an answer that cannot be read gives
    /// [`PageStatus::Refused`] or [`PageStatus::Unreadable`], and the next
    /// call asks again. An item of the
    /// answer with an error leaves its values as they are. A busy receiver can
    /// leave items out of the answer: the items that are there are used, and
    /// the values of the others stay as they are. A receiver that is
    /// not calibrated with Audyssey answers with blank Audyssey values, which
    /// count as unknown. While telnet is open, a blank value does not clear
    /// a value that telnet reported.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiver does not answer in time or the
    /// connection fails, see [`Error::is_unreachable`].
    pub async fn update_signal_and_settings(&self) -> Result<PageStatus, Error> {
        self.in_span(self.update_signal_and_settings_in()).await
    }

    /// Does the work of [`Avr::update_signal_and_settings`].
    async fn update_signal_and_settings_in(&self) -> Result<PageStatus, Error> {
        self.update_signal_and_settings_with_channels()
            .await
            .map(|(status, _)| status)
    }

    /// Like [`Avr::update_signal_and_settings`]. Also gives true if the answer
    /// has items for the channels of the signal and of the speakers that are
    /// not error items.
    pub(crate) async fn update_signal_and_settings_with_channels(
        &self,
    ) -> Result<(PageStatus, bool), Error> {
        self.read_appcommand0300(&self.appcommand0300_commands, &self.appcommand0300_body)
            .await
            .map(|(status, answered)| (status, answered.has_channels()))
    }

    /// The commands of the first `AppCommand0300.xml` request of the last
    /// start that were answered without an error item.
    fn signal_answered(&self) -> Answered0300 {
        Answered0300(self.signal_answered.load(Ordering::Relaxed))
    }

    /// The last start read the channels of the signal and of the speakers
    /// with `AppCommand0300.xml`.
    pub(crate) fn channels_from_appcommand(&self) -> bool {
        self.signal_answered().has_channels()
    }

    /// The last start read the audio signal with `AppCommand0300.xml`.
    #[cfg(feature = "telnet")]
    pub(crate) fn audio_info_from_appcommand(&self) -> bool {
        self.signal_answered().has(&appcommand::GET_AUDIO_INFO)
    }

    /// Like [`Avr::update_signal_and_settings`], in the two requests of
    /// [`APPCOMMAND0300_FIRST`], so that the audio signal and the channels
    /// come soon. The status is [`PageStatus::Read`] if one request was read.
    /// Else it is the status of the first request.
    pub(crate) async fn update_signal_and_settings_in_parts(&self) -> Result<PageStatus, Error> {
        self.signal_answered.store(0, Ordering::Relaxed);
        let mut status = None;
        // The commands of the first request that were answered.
        let mut first_answered = Answered0300::default();
        for (index, part) in self.appcommand0300_parts.iter().enumerate() {
            if part.commands.is_empty() {
                continue;
            }
            let (part_status, part_answered) =
                match self.read_appcommand0300(&part.commands, &part.body).await {
                    Ok(result) => result,
                    // The values of the request that was read count.
                    Err(error) if status == Some(PageStatus::Read) => {
                        tracing::debug!(host = %self.info.host, %error, "AppCommand0300 not read");
                        (PageStatus::NoAnswer, Answered0300::default())
                    }
                    Err(error) => return Err(error),
                };
            if index == 0 {
                first_answered = part_answered;
            }
            if status.is_none() || part_status == PageStatus::Read {
                status = Some(part_status);
            }
            // The receiver does not have the file.
            if part_status == PageStatus::NotFound {
                break;
            }
        }
        // The start asks telnet again for what an error item left out. The
        // values of the second request do not change what it asks.
        self.signal_answered
            .store(first_answered.0, Ordering::Relaxed);
        Ok(status.unwrap_or(PageStatus::NotFound))
    }

    /// Sends `body`, the request for `commands`, to `AppCommand0300.xml` and
    /// puts the answer into the state. Also gives the commands that were
    /// answered without an error item.
    async fn read_appcommand0300(
        &self,
        commands: &[Definition],
        body: &Bytes,
    ) -> Result<(PageStatus, Answered0300), Error> {
        if self.info.update_method != UpdateMethod::AppCommand
            || self.missing.appcommand0300.load(Ordering::Relaxed)
        {
            return Ok((PageStatus::NotFound, Answered0300::default()));
        }
        // Telnet reports the Audyssey values and the HDMI output, so a blank
        // one must not clear them.
        let telnet_open = self.telnet_connected();
        let result = self
            .http
            .post_parsed(Endpoint::AppCommand0300.path(), body.clone(), |body| {
                // A busy receiver can leave items out.
                let response = Response::parse_at_most(body, commands.len())?;
                let mut answered = Answered0300::default();
                self.channels.modify(|state, notify| {
                    for (index, item) in response.matched(commands) {
                        // An error item has no values, and must not clear
                        // what the state has.
                        if item.is_error() {
                            continue;
                        }
                        let definition = &commands[index];
                        answered.add(definition);
                        let keep_known = telnet_open
                            && (*definition == appcommand::GET_AUDYSSEY
                                || *definition == appcommand::GET_OUTPUT_SETTINGS);
                        definition.extract(&item, Zone::Main, &mut |field, value| {
                            if value.is_some() || !keep_known {
                                notify(state.apply_field(Zone::Main, field, value));
                            }
                        });
                        if *definition == appcommand::GET_INPUT_SIGNAL {
                            notify(state.apply_channel_item(true, &item));
                        } else if *definition == appcommand::GET_ACTIVE_SPEAKER {
                            notify(state.apply_channel_item(false, &item));
                        }
                    }
                });
                if answered.count() as usize != commands.len() {
                    tracing::debug!(
                        host = %self.info.host,
                        answered = answered.count(),
                        asked = commands.len(),
                        "AppCommand0300 answer has no values for some commands"
                    );
                }
                Ok(answered)
            })
            .await;
        match result {
            Ok(answered) => {
                self.forget_telnet_values(&Ok(PageStatus::Read));
                // Only the video values can make BT.2020 not known.
                if commands.contains(&appcommand::GET_VIDEO_INFO) {
                    self.ask_colorimetry_if_unknown().await;
                }
                Ok((PageStatus::Read, answered))
            }
            // A timeout or a network error can pass.
            Err(error) if error.is_unreachable() => Err(error),
            Err(error) => {
                tracing::debug!(host = %self.info.host, %error, "AppCommand0300 not read");
                let status = match error {
                    // Only a missing file means that the receiver does not have it.
                    Error::Status { status: 404, .. } => {
                        self.missing.appcommand0300.store(true, Ordering::Relaxed);
                        PageStatus::NotFound
                    }
                    Error::Status { status, .. } => PageStatus::Refused(status),
                    Error::Forbidden { .. } => PageStatus::Refused(403),
                    _ => PageStatus::Unreadable,
                };
                Ok((status, Answered0300::default()))
            }
        }
    }

    /// Reads the settings of `part` from `AppCommand0300.xml`, the queries
    /// that `Deviceinfo.xml` lists. Telnet gives the same values, so nothing
    /// is sent while telnet is connected. A value that the answer leaves
    /// blank does not clear what the state has. Returns the number of
    /// commands that were sent, `0` if nothing was sent.
    async fn read_settings(&self, part: SettingsPart) -> Result<usize, Error> {
        if self.telnet_connected() {
            return Ok(0);
        }
        let Some(request) = settings_request(&self.info, part) else {
            return Ok(0);
        };
        let count = request.1;
        let result = read_settings_request(
            &self.http,
            &self.channels,
            request,
            self.info.brand,
            self.telnet_groups(),
            || self.telnet_connected(),
            || self.forget_telnet_values(&Ok(PageStatus::Read)),
        )
        .await;
        if let Err(error) = &result {
            tracing::debug!(host = %self.info.host, ?part, %error, "settings not read");
        }
        result.map(|()| count)
    }

    /// The groups that telnet asks for after a connect. None without telnet.
    #[cfg_attr(not(feature = "telnet"), allow(clippy::unused_self))]
    const fn telnet_groups(&self) -> &'static [QueryGroup] {
        #[cfg(feature = "telnet")]
        return self.telnet.query_groups();
        #[cfg(not(feature = "telnet"))]
        &[]
    }

    /// Returns true if telnet is connected now.
    #[cfg_attr(
        not(feature = "telnet"),
        allow(clippy::missing_const_for_fn, clippy::unused_self)
    )]
    fn telnet_connected(&self) -> bool {
        #[cfg(feature = "telnet")]
        return self.telnet.is_connected();
        #[cfg(not(feature = "telnet"))]
        false
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

/// Sends each query with `send`, in order, and stops at the first error.
pub(crate) async fn send_queries(
    queries: &[&'static str],
    send: impl AsyncFn(Command<'static>) -> Result<(), Error>,
) -> Result<(), Error> {
    for query in queries {
        send(Command::Raw(query)).await?;
    }
    Ok(())
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
            // The source lists are for the whole receiver.
            if zone == Zone::Main {
                status_page::sources(
                    status.as_ref(),
                    zone_page.as_ref(),
                    |name, rename, in_use| {
                        notify(state.apply_renamed_source(name, rename));
                        if let Some(in_use) = in_use {
                            notify(state.apply_source_in_use(name, in_use));
                        }
                    },
                );
            }
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

    /// Sets the time after a command in which background work waits, so that
    /// the receiver does the command first. The default is 250 ms.
    ///
    /// On telnet, the queries that fill the state wait after a command from
    /// the caller. Over HTTP, reads wait while a command over HTTP waits or
    /// runs, and for this time after it ends. The read after a command (see
    /// [`AvrBuilder::follow_up_read`]) does not wait for this time. A command
    /// on telnet does not make HTTP reads wait.
    #[must_use]
    pub const fn background_pause(mut self, pause: Duration) -> Self {
        self.background_pause = pause;
        self
    }

    /// Sets the port of the web interface. The default is 11080.
    #[must_use]
    pub const fn web_interface_port(mut self, port: u16) -> Self {
        self.web_interface_port = port;
        self
    }

    /// Sets the types and ports to try when identifying the receiver, see
    /// [`identify_on`]. The default is the usual ports of each receiver type.
    /// Use this if the AppCommand port is not a usual one, for example behind
    /// a proxy.
    ///
    /// To connect faster, pass the [`Info::receiver_type`] and [`Info::port`]
    /// of an earlier connection as the only entry. Then only one request is
    /// sent. If the receiver is now of a different type, `Info` shows other
    /// values than the ones passed; connect again without probes to look up
    /// the type again.
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

    /// Sets the brand to use if the receiver does not give one, for example
    /// the [`Info::brand`] of an earlier connection. The brand in the device
    /// description or the device information wins. The default is
    /// [`Brand::Denon`].
    #[must_use]
    pub const fn brand(mut self, brand: Brand) -> Self {
        self.brand = Some(brand);
        self
    }

    /// Uses the [`Info`] of an earlier connection, so that
    /// [`AvrBuilder::connect`] sends no request and commands can go out at
    /// once. The host of the builder replaces [`Info::host`]. The probes,
    /// the description port and the brand are not used then.
    ///
    /// [`Avr::start`] checks the values first and gives the result in
    /// [`Started::info_check`]; [`Avr::verify`] checks them without a
    /// start. If the receiver gives other values, connect again with the new
    /// [`Info`], or without this to look up everything again.
    ///
    /// A firmware update can change [`Info::features`] and [`Info::ranges`].
    /// The check finds such a change too.
    ///
    /// [`AvrBuilder::missing`] also skips the sources that the receiver does
    /// not have.
    #[must_use]
    pub fn known(mut self, info: Info) -> Self {
        self.known = Some(info);
        self
    }

    /// Uses the [`Avr::missing_sources`] of an earlier connection with the
    /// same [`Info`], so that these sources are not asked again. Used only
    /// with [`AvrBuilder::known`]: a connect without it starts with no
    /// missing source. When [`Started::info_check`] or [`Avr::verify`]
    /// gives changed values, drop the stored value and connect without
    /// this.
    #[must_use]
    pub const fn missing(mut self, missing: MissingSources) -> Self {
        self.missing = missing;
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

    /// Settings of the HEOS connection, see [`Avr::heos_connect`].
    #[cfg(feature = "heos")]
    #[must_use]
    pub const fn heos_config(mut self, config: denon_marantz_heos::HeosConfig) -> Self {
        self.heos = config;
        self
    }

    /// Identifies the receiver and reads what it tells about itself. With
    /// [`AvrBuilder::known`], it uses the values given there and sends no
    /// request.
    ///
    /// The state is empty until [`Avr::update`] runs.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiver cannot be reached, and
    /// [`Error::InvalidConfig`] if the telnet or HEOS settings are not valid.
    /// A missing or unusable device description is not an error: the brand is
    /// then the one from the device information, [`AvrBuilder::brand`] or
    /// [`Brand::Denon`], and the model is not known.
    pub async fn connect(self) -> Result<Avr, Error> {
        let span = crate::log::session(&self.host);
        self.connect_in(span.clone()).instrument(span).await
    }

    /// Does the work of [`AvrBuilder::connect`]. The clients and tasks of the
    /// receiver log in `span`.
    async fn connect_in(mut self, span: Span) -> Result<Avr, Error> {
        #[cfg(feature = "telnet")]
        self.telnet.check()?;
        #[cfg(feature = "heos")]
        self.heos.check().map_err(|error| match error {
            denon_marantz_heos::Error::InvalidConfig(text) => Error::InvalidConfig(text),
            other => Error::Heos(other),
        })?;
        let priority = Arc::new(Priority::new(self.background_pause));
        if self.known.is_none() && self.missing != MissingSources::default() {
            tracing::debug!(host = %self.host, "missing sources are not used without known values");
        }
        if let Some(mut info) = self.known.take() {
            info.host.clone_from(&self.host);
            let http = HttpClient::new(&self.host, info.port, self.http)?
                .with_priority(priority.clone())
                .with_span(span);
            tracing::info!(host = %info.host, name = %info.friendly_name, method = ?info.update_method, "connected with known values");
            return self.assemble(info, http, priority, true);
        }
        let identified = match &self.probes {
            Some(probes) => identify_on(&self.host, probes, self.http).await?,
            None => identify(&self.host, self.http).await?,
        };
        let http = HttpClient::new(&self.host, identified.port, self.http)?
            .with_priority(priority.clone())
            .with_span(span);
        // The brand from the receiver wins over the given one.
        let brand = identified.device_info.brand.or(self.brand);
        let description = self.description(identified.receiver_type, brand).await?;
        let (update_method, friendly_name) =
            detect_update_method(&http, identified.receiver_type).await?;
        let (brand, friendly_name) =
            brand_and_name(&self.host, description.as_ref(), brand, friendly_name);
        let info = info_from(
            self.host.clone(),
            identified,
            update_method,
            brand,
            friendly_name,
            description.as_ref(),
        );
        tracing::info!(host = %info.host, name = %info.friendly_name, method = ?info.update_method, "connected");
        self.assemble(info, http, priority, false)
    }

    /// Builds the receiver from what is known about it.
    fn assemble(
        self,
        info: Info,
        http: HttpClient,
        priority: Arc<Priority>,
        known: bool,
    ) -> Result<Avr, Error> {
        let appcommand0300_commands = appcommand0300_commands(info.queries.as_ref());
        let channels = Channels::new(State::new(info.single_zone()));
        let missing = Arc::new(Missing::new(
            info.receiver_type,
            info.queries.as_ref(),
            appcommand0300_commands.is_empty(),
            if known {
                self.missing
            } else {
                MissingSources::default()
            },
        ));
        let update_cmds = UPDATE_COMMANDS.map(|definition| definition.cmd);
        let appcommand0300_body = definitions_body(&appcommand0300_commands)?;
        let part = |first: bool| {
            Request0300::new(
                appcommand0300_commands
                    .iter()
                    .filter(|definition| APPCOMMAND0300_FIRST.contains(definition) == first)
                    .copied()
                    .collect(),
            )
        };
        let appcommand0300_parts = [part(true)?, part(false)?];
        #[cfg(feature = "telnet")]
        let telnet = Telnet::new(
            self.telnet,
            self.background_pause,
            &info,
            http.span().clone(),
        );
        let follow_up = FollowUp::start(
            self.follow_up_read,
            &http,
            &channels,
            &info,
            &missing,
            &appcommand0300_commands,
            #[cfg(feature = "telnet")]
            &telnet,
        )?;
        #[cfg(feature = "heos")]
        let heos = HeosOwner::new(HeosLink::new(
            info.host.clone(),
            info.serial_number.clone(),
            self.heos,
            info.has_heos(),
            http.span().clone(),
        ));
        let web_interface =
            HttpClient::new(&info.host, self.web_interface_port, self.web_interface)?
                .with_priority(priority.clone())
                .with_span(http.span().clone());
        Ok(Avr {
            http,
            web_interface,
            info,
            update_body: request_body(&update_cmds)?,
            update_body_without_tone: request_body(
                &update_cmds[..UPDATE_COMMANDS_WITHOUT_TONE.len()],
            )?,
            #[cfg(feature = "telnet")]
            http_only_body: request_body(&HTTP_ONLY_COMMANDS.map(|definition| definition.cmd))?,
            #[cfg(feature = "telnet")]
            http_only_with_device_body: request_body(
                &HTTP_ONLY_WITH_DEVICE_COMMANDS.map(|definition| definition.cmd),
            )?,
            appcommand0300_commands,
            appcommand0300_body,
            appcommand0300_parts,
            missing,
            signal_answered: AtomicU8::new(0),
            channels,
            starting: AtomicBool::new(false),
            #[cfg(feature = "telnet")]
            telnet,
            #[cfg(feature = "heos")]
            heos,
            reset_keys: ResetKeys::default(),
            priority,
            follow_up,
            known,
        })
    }

    /// Reads the device description, with one more try if the first one did
    /// not time out. `None` if the receiver has no usable one. The receiver
    /// was identified before, so a failure here is not an error: the
    /// description only adds the brand and the model. `brand` is only for the
    /// warning.
    async fn description(
        &self,
        receiver_type: ReceiverType,
        brand: Option<Brand>,
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
                // A timeout is not tried again, so a port that does not
                // answer costs one timeout.
                Err(error) => {
                    let timeout = error.is_timeout();
                    last_error = Some(error);
                    if timeout {
                        break;
                    }
                }
            }
        }
        if let Some(error) = last_error {
            let brand = brand.unwrap_or(Brand::Denon);
            tracing::warn!(host = %self.host, %error, "no device description, assuming a {} receiver", brand.name());
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

/// The info from the device information and the answers after it.
fn info_from(
    host: String,
    identified: Identified,
    update_method: UpdateMethod,
    brand: Brand,
    friendly_name: String,
    description: Option<&DeviceDescription<'_>>,
) -> Info {
    let Identified {
        receiver_type,
        port,
        device_info,
        type_proof,
    } = identified;
    Info {
        host,
        receiver_type,
        port,
        type_proof,
        update_method,
        brand,
        manufacturer: description.map(|d| d.manufacturer.to_string()),
        model_name: description.and_then(|d| owned(d.model_name.as_ref())),
        serial_number: description.and_then(|d| owned(d.serial_number.as_ref())),
        friendly_name,
        comm_api_version: owned(device_info.comm_api_version.as_ref()),
        mac_address: owned(device_info.mac_address.as_ref()),
        dirac_live: device_info.dirac_live,
        tuner: device_info.tuner,
        quick_select_count: device_info.quick_select_count,
        queries: device_info.queries,
        zones: listed_zones(device_info.device_zones),
        zone_count: device_info.device_zones,
        features: device_info.features,
        ranges: device_info.ranges,
    }
}

/// The brand from the description, or else the fallback (from the device
/// information or the caller), or else [`Brand::Denon`]. The name that the
/// receiver gave, or else the one from the description, or else a name made
/// from the brand.
fn brand_and_name(
    host: &str,
    description: Option<&DeviceDescription<'_>>,
    fallback: Option<Brand>,
    friendly_name: Option<String>,
) -> (Brand, String) {
    let brand = description
        .filter(|d| !d.manufacturer.is_empty())
        .map(DeviceDescription::brand)
        .or(fallback)
        .unwrap_or_else(|| Brand::from_manufacturer(None));
    if let Some(fallback) = fallback
        && fallback != brand
    {
        tracing::debug!(
            host,
            fallback = fallback.name(),
            brand = brand.name(),
            "the device description names another brand"
        );
    }
    let name = friendly_name
        .or_else(|| description.and_then(|d| owned(d.friendly_name.as_ref())))
        .unwrap_or_else(|| format!("{} {DEFAULT_NAME}", brand.name()));
    (brand, name)
}

/// The request body of these commands.
fn definitions_body(definitions: &[Definition]) -> Result<Bytes, Error> {
    let cmds: Vec<_> = definitions
        .iter()
        .map(|definition| definition.cmd)
        .collect();
    request_body(&cmds)
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
            tracing::debug!(host = http.authority(), %error, "status page not available");
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

/// The rule of [`Info::offers`]: what the spec generation and the device
/// information say about the values of a receiver.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Offered {
    generation: SpecGeneration,
    features: Option<DeviceFeatures>,
    /// For zone 2 and zone 3: see [`Info::has_zone`].
    other_zones: [bool; 2],
}

impl Offered {
    /// See [`Info::offers`].
    pub(crate) fn offers(self, query: Query) -> bool {
        query
            .spec_feature()
            .is_none_or(|feature| self.generation.supports(feature))
            && self.lists(query)
    }

    /// [`Offered::offers`] for the value of the query text. True for a text
    /// that is not in the query lists.
    #[cfg(feature = "telnet")]
    pub(crate) fn offers_text(self, text: &str) -> bool {
        denon_marantz_proto::telnet::query_for_text(text).is_none_or(|query| self.offers(query))
    }

    /// See [`Info::lists`].
    const fn lists(self, query: Query) -> bool {
        self.has_zone(query.zone()) && query.listed_in(self.features)
    }

    const fn has_zone(self, zone: Zone) -> bool {
        match zone {
            Zone::Zone2 => self.other_zones[0],
            Zone::Zone3 => self.other_zones[1],
            _ => true,
        }
    }
}

/// The zones for the zone count of the device information: the main zone
/// only if it does not say.
fn listed_zones(count: Option<u8>) -> Vec<Zone> {
    Zone::ALL[..usize::from(count.unwrap_or(1)).clamp(1, Zone::ALL.len())].to_vec()
}

/// Info of a receiver for tests. `zones` is the zone count of the device
/// information.
#[cfg(test)]
pub(crate) fn test_info(version: &str, brand: Brand, zones: Option<u8>) -> Info {
    Info {
        host: "receiver".into(),
        receiver_type: ReceiverType::AvrX2016,
        port: 8080,
        type_proof: TypeProof::Read,
        update_method: UpdateMethod::AppCommand,
        brand,
        manufacturer: None,
        model_name: None,
        serial_number: None,
        friendly_name: String::new(),
        comm_api_version: Some(version.into()),
        mac_address: None,
        dirac_live: false,
        tuner: TunerInfo::Unknown,
        quick_select_count: None,
        queries: None,
        zones: listed_zones(zones),
        zone_count: zones,
        features: None,
        ranges: DeviceRanges::default(),
    }
}

#[cfg(test)]
mod tests {
    use denon_marantz_proto::{DeviceFeature, RemoteKey, SoundSettingValue, ValueRange};

    use super::*;

    #[test]
    fn generation_hides_older_commands() {
        let current = test_info("0301", Brand::Denon, Some(2));
        assert_eq!(current.spec_generation(), SpecGeneration::Current);
        assert!(current.has_heos());
        assert!(!current.supports(SpecFeature::ProLogicMode));
        assert!(!current.offers(Query::ProLogicMode));
        assert!(!current.offers_line("PSMODE:MUSIC"));
        assert!(current.offers_line("PSLFL 05"));
        let old = test_info("0210", Brand::Marantz, Some(2));
        assert!(old.offers(Query::DynamicCompression));
        assert!(!old.offers(Query::SurroundBackMode));
        assert!(!old.offers_line("PSDIL ON"));
        let unknown = test_info("", Brand::Denon, None);
        assert!(unknown.offers(Query::AutoFlagDetect));
    }

    #[test]
    fn zone_lines_need_the_zone() {
        let one = test_info("0301", Brand::Denon, Some(1));
        let two = test_info("0301", Brand::Denon, Some(2));
        let unknown = test_info("0301", Brand::Denon, None);
        for line in ["Z2PSBAS 50", "Z2MU ON", "Z250", "Z2ON"] {
            assert!(!one.offers_line(line), "{line}");
            assert!(two.offers_line(line), "{line}");
            assert!(unknown.offers_line(line), "{line}");
        }
        for line in ["Z3MU ON", "Z3PSTRE 50"] {
            assert!(!two.offers_line(line), "{line}");
            assert!(unknown.offers_line(line), "{line}");
        }
        assert!(one.offers_line("MUON"));
        let key = Command::RemoteKey(RemoteKey::PageUp);
        assert!(!one.offers_command(Zone::Zone3, &key));
        assert!(unknown.offers_command(Zone::Zone3, &key));
        assert!(!one.offers(Query::Zone2Bass));
        assert!(!one.offers_command(Zone::Zone2, &Command::Mute(true)));
        assert!(two.offers_command(Zone::Zone2, &Command::Mute(true)));
    }

    #[test]
    fn listed_features_decide() {
        let mut info = test_info("0301", Brand::Denon, Some(3));
        info.features = Some(DeviceFeatures::default().with(DeviceFeature::Lfe));
        assert!(!info.offers_line("BTLEV UP"));
        assert!(!info.offers(Query::BluetoothLevel));
        assert!(!info.offers_line("Z3MU ON"));
        assert!(info.offers_line("PSLFE 05"));
        info.features = Some(DeviceFeatures::default().with(DeviceFeature::Bttx));
        assert!(info.offers_line("BTLEV UP"));
    }

    #[test]
    fn commands_check_ranges_and_network_audio() {
        let mut info = test_info("0300", Brand::Denon, Some(1));
        let lfe = |value| Command::SetSoundSetting(SoundSettingValue::LfeExternalInput(value));
        assert!(info.offers_command(Zone::Main, &lfe(15)));
        info.ranges.lfe_external_input = Some(ValueRange {
            min_tenths: 0,
            max_tenths: 100,
            step_tenths: Some(50),
        });
        assert!(info.offers_command(Zone::Main, &lfe(10)));
        assert!(!info.offers_command(Zone::Main, &lfe(15)));
        let next = Command::NetworkAudio(NetworkAudioCommand::Next);
        let preset = |number| Command::NetworkAudio(NetworkAudioCommand::Preset(number));
        let store = |number| Command::NetworkAudio(NetworkAudioCommand::StorePreset(number));
        assert!(info.offers_command(Zone::Main, &next));
        for generation in [SpecGeneration::Denon2015, SpecGeneration::Unknown] {
            info.comm_api_version = Some(if generation == SpecGeneration::Unknown {
                "0220".into()
            } else {
                "0300".into()
            });
            let presets = generation.network_presets().unwrap();
            assert!(info.offers_command(Zone::Main, &preset(presets)));
            assert!(!info.offers_command(Zone::Main, &preset(presets + 1)));
            assert!(!info.offers_command(Zone::Main, &store(presets + 1)));
        }
        info.comm_api_version = Some("0301".into());
        assert!(!info.offers_command(Zone::Main, &next));
        assert!(!info.offers_command(Zone::Main, &preset(1)));
        assert!(!info.offers_command(Zone::Main, &Command::Raw("")));
    }

    #[test]
    fn quick_select_slots_follow_the_count() {
        let mut info = test_info("0301", Brand::Denon, Some(1));
        assert!(!info.has_quick_select(0));
        assert!(info.has_quick_select(MAX_QUICK_SELECT));
        assert!(!info.has_quick_select(MAX_QUICK_SELECT + 1));
        info.quick_select_count = Some(4);
        assert!(info.has_quick_select(4));
        assert!(!info.has_quick_select(5));
        let select = |slot| Command::SetZoneSetting(ZoneSettingValue::QuickSelect(slot));
        let store = |slot| Command::SetZoneSetting(ZoneSettingValue::StoreQuickSelect(slot));
        assert!(info.offers_command(Zone::Main, &select(4)));
        assert!(!info.offers_command(Zone::Main, &select(5)));
        assert!(!info.offers_command(Zone::Main, &store(5)));
        info.quick_select_count = Some(0);
        assert!(!info.has_quick_select(1));
    }

    #[test]
    fn http_reads_follow_the_lists() {
        let mut info = test_info("0301", Brand::Denon, Some(2));
        let mut listed = ListedQueries::default();
        listed.surround_parameter = true;
        listed.zone_tone = true;
        info.queries = Some(listed);
        for query in [
            Query::Volume,
            Query::Zone2Power,
            Query::Dimmer,
            Query::CinemaEq,
            Query::Zone2Bass,
            Query::HdrFormat,
            Query::VideoResolution,
        ] {
            assert!(info.readable_over_http(query), "{query:?}");
        }
        for query in [
            Query::Zone3Power,
            Query::Zone3Bass,
            Query::MultEq,
            Query::SamplingRate,
            Query::HdmiOutput,
            Query::HdmiMaxResolution,
            Query::DialogEnhancer,
            Query::ToneControl,
            Query::ProLogicMode,
            Query::Menu,
        ] {
            assert!(!info.readable_over_http(query), "{query:?}");
        }
        assert!(info.readable_over_http(Query::ExtendedColorimetry));
        assert!(!info.readable_over_http(Query::AutoLipSyncAls));
        let mut delay = info.clone();
        if let Some(listed) = delay.queries.as_mut() {
            listed.audio_delay = true;
        }
        assert!(delay.readable_over_http(Query::AutoLipSyncAls));
        let mut three = info.clone();
        three.zones = Zone::ALL.to_vec();
        three.zone_count = Some(3);
        assert!(three.readable_over_http(Query::Zone3Bass));
        let mut no_count = info.clone();
        no_count.zones = vec![Zone::Main];
        no_count.zone_count = None;
        assert!(no_count.offers(Query::Zone2Power));
        assert!(!no_count.readable_over_http(Query::Zone2Power));
        info.update_method = UpdateMethod::StatusPages;
        assert!(info.readable_over_http(Query::SourceNames));
        assert!(info.readable_over_http(Query::Volume));
        assert!(info.readable_over_http(Query::HdrFormat));
        assert!(!info.readable_over_http(Query::Dimmer));
        assert!(!info.readable_over_http(Query::CinemaEq));
        assert!(!info.readable_over_http(Query::SamplingRate));
    }

    #[test]
    fn http_reads_name_the_methods() {
        const UPDATE: HttpReads = HttpReads::UPDATE.union(HttpReads::REFRESH);
        const APPCOMMAND0300: HttpReads =
            HttpReads::SIGNAL_AND_SETTINGS.union(HttpReads::SIGNAL_PAGES);
        let mut info = test_info("0301", Brand::Denon, Some(2));
        let mut listed = ListedQueries::default();
        listed.tone_control = true;
        listed.audyssey = true;
        listed.video_info = true;
        listed.input_signal = true;
        listed.surround_parameter = true;
        info.queries = Some(listed);
        for (query, expected) in [
            (Query::Volume, UPDATE),
            (Query::Zone2Power, UPDATE),
            (Query::Dimmer, UPDATE),
            (Query::Bass, UPDATE),
            (Query::MultEq, APPCOMMAND0300),
            (Query::VideoResolution, APPCOMMAND0300),
            (Query::VideoInput, APPCOMMAND0300),
            (Query::InputChannels, APPCOMMAND0300),
            (Query::OutputChannels, HttpReads::SIGNAL_PAGES),
            (Query::HdrFormat, HttpReads::SIGNAL_PAGES),
            (Query::ExtendedColorimetry, HttpReads::SIGNAL_PAGES),
            (Query::CinemaEq, HttpReads::REFRESH),
            (Query::Zone2Bass, HttpReads::EMPTY),
            (Query::SamplingRate, HttpReads::EMPTY),
            (Query::HdmiOutput, HttpReads::EMPTY),
            (Query::Zone3Power, HttpReads::EMPTY),
            (Query::Menu, HttpReads::EMPTY),
        ] {
            assert_eq!(info.http_reads(query), expected, "{query:?}");
        }
        assert_eq!(
            info.http_reads(Query::AutoLipSyncAls),
            info.http_reads(Query::AutoLipSync)
        );
        if let Some(listed) = info.queries.as_mut() {
            listed.audio_info = true;
            listed.zone_tone = true;
            listed.output_settings = true;
        }
        assert_eq!(info.http_reads(Query::SamplingRate), APPCOMMAND0300);
        assert_eq!(info.http_reads(Query::Zone2Bass), HttpReads::REFRESH);
        assert_eq!(info.http_reads(Query::HdmiOutput), APPCOMMAND0300);
        // Without lists all but the HDMI output are asked, and no settings.
        let mut unlisted = info.clone();
        unlisted.queries = None;
        for (query, expected) in [
            (Query::Bass, UPDATE),
            (Query::MultEq, APPCOMMAND0300),
            (Query::SamplingRate, APPCOMMAND0300),
            (Query::OutputChannels, APPCOMMAND0300),
            (Query::HdmiOutput, HttpReads::EMPTY),
            (Query::CinemaEq, HttpReads::EMPTY),
        ] {
            assert_eq!(unlisted.http_reads(query), expected, "{query:?}");
        }
        info.update_method = UpdateMethod::StatusPages;
        for (query, expected) in [
            (Query::Volume, UPDATE),
            (Query::Dimmer, HttpReads::EMPTY),
            (Query::MultEq, HttpReads::EMPTY),
            (Query::HdmiOutput, HttpReads::EMPTY),
            (Query::VideoResolution, HttpReads::SIGNAL_PAGES),
            (Query::InputChannels, HttpReads::SIGNAL_PAGES),
            (Query::CinemaEq, HttpReads::EMPTY),
        ] {
            assert_eq!(info.http_reads(query), expected, "{query:?}");
        }
    }

    #[test]
    fn signal_and_settings_follow_the_request() {
        type Set = fn(&mut ListedQueries);
        let audio_info = |query: Query| query.signal_part() == Some(SignalPart::AudioInfo);
        for &query in Query::ALL {
            assert_eq!(
                audio_info(query),
                appcommand0300_definition(query) == Some(appcommand::GET_AUDIO_INFO),
                "{query:?}"
            );
        }
        // Each command of the request gives the value of some query.
        for definition in APPCOMMAND0300_COMMANDS {
            assert!(
                Query::ALL
                    .iter()
                    .any(|&query| appcommand0300_definition(query) == Some(definition)),
                "{:?}",
                definition.cmd.name
            );
        }
        let lists: [Set; 7] = [
            |_| {},
            |listed| listed.audyssey = true,
            |listed| listed.video_info = true,
            |listed| listed.audio_info = true,
            |listed| listed.input_signal = true,
            |listed| listed.active_speaker = true,
            |listed| listed.output_settings = true,
        ];
        let mut info = test_info("0301", Brand::Denon, Some(3));
        let mut infos = vec![info.clone()];
        for set in lists {
            let mut listed = ListedQueries::default();
            set(&mut listed);
            info.queries = Some(listed);
            infos.push(info.clone());
        }
        for info in &mut infos {
            for update_method in [UpdateMethod::AppCommand, UpdateMethod::StatusPages] {
                info.update_method = update_method;
                let sent = appcommand0300_commands(info.queries.as_ref());
                for &query in Query::ALL {
                    let expected = update_method == UpdateMethod::AppCommand
                        && info.offers(query)
                        && info.zones.contains(&query.zone())
                        && appcommand0300_definition(query)
                            .is_some_and(|definition| sent.contains(&definition));
                    let reads = info.http_reads(query);
                    // SIGNAL_PAGES reads all that SIGNAL_AND_SETTINGS reads.
                    assert!(
                        !reads.contains(HttpReads::SIGNAL_AND_SETTINGS)
                            || reads.contains(HttpReads::SIGNAL_PAGES),
                        "{query:?}"
                    );
                    assert_eq!(
                        reads.contains(HttpReads::SIGNAL_AND_SETTINGS),
                        expected,
                        "{query:?} {:?} {update_method:?}",
                        info.queries
                    );
                }
            }
        }
    }

    #[test]
    fn http_reads_leave_out_missing_sources() {
        const UPDATE: HttpReads = HttpReads::UPDATE.union(HttpReads::REFRESH);
        const BOTH: HttpReads = HttpReads::SIGNAL_AND_SETTINGS.union(HttpReads::SIGNAL_PAGES);
        const PAGES: HttpReads = HttpReads::SIGNAL_PAGES;
        const NONE: HttpReads = HttpReads::EMPTY;
        let mut info = test_info("0301", Brand::Denon, Some(2));
        let mut listed = ListedQueries::default();
        listed.tone_control = true;
        listed.audyssey = true;
        listed.input_signal = true;
        listed.surround_parameter = true;
        info.queries = Some(listed);
        let without = |missing| {
            [
                Query::Bass,
                Query::MultEq,
                Query::InputChannels,
                Query::HdrFormat,
                Query::ExtendedColorimetry,
                Query::CinemaEq,
            ]
            .map(|query| info.http_reads_without(query, missing))
        };
        let refresh = HttpReads::REFRESH;
        for (missing, expected) in [
            (
                MissingSources::default(),
                [UPDATE, BOTH, BOTH, PAGES, PAGES, refresh],
            ),
            (
                MissingSources {
                    tone_control: true,
                    ..MissingSources::default()
                },
                [NONE, BOTH, BOTH, PAGES, PAGES, refresh],
            ),
            // The settings request is kept, see `MissingSources`.
            (
                MissingSources {
                    appcommand0300: true,
                    ..MissingSources::default()
                },
                [UPDATE, NONE, PAGES, PAGES, PAGES, refresh],
            ),
            (
                MissingSources {
                    video_page: true,
                    channels_page: true,
                    ..MissingSources::default()
                },
                [UPDATE, BOTH, BOTH, NONE, NONE, refresh],
            ),
            (
                MissingSources {
                    appcommand0300: true,
                    channels_page: true,
                    ..MissingSources::default()
                },
                [UPDATE, NONE, NONE, PAGES, PAGES, refresh],
            ),
        ] {
            assert_eq!(without(missing), expected, "{missing:?}");
        }
    }

    #[test]
    fn answered_0300_has_the_added_commands() {
        let mut answered = Answered0300::default();
        answered.add(&appcommand::GET_INPUT_SIGNAL);
        assert!(!answered.has_channels());
        answered.add(&appcommand::GET_ACTIVE_SPEAKER);
        answered.add(&appcommand::GET_DIMMER);
        assert!(answered.has_channels());
        assert!(!answered.has(&appcommand::GET_AUDIO_INFO));
        assert!(!answered.has(&appcommand::GET_DIMMER));
        assert_eq!(answered.count(), 2);
    }

    #[test]
    fn http_reads_combine() {
        let both = HttpReads::UPDATE | HttpReads::SIGNAL_PAGES;
        assert!(HttpReads::EMPTY.is_empty());
        assert_eq!(HttpReads::default(), HttpReads::EMPTY);
        assert!(!both.is_empty());
        assert!(both.contains(HttpReads::UPDATE));
        assert!(both.contains(HttpReads::EMPTY));
        assert!(!both.contains(HttpReads::UPDATE | HttpReads::REFRESH));
        assert!(both.intersects(HttpReads::SIGNAL_PAGES | HttpReads::REFRESH));
        assert!(!both.intersects(HttpReads::REFRESH | HttpReads::SIGNAL_AND_SETTINGS));
        assert!(!both.intersects(HttpReads::EMPTY));
        let mut reads = HttpReads::EMPTY;
        reads |= HttpReads::UPDATE;
        reads |= HttpReads::SIGNAL_PAGES;
        assert_eq!(reads, both);
        assert_eq!(format!("{both:?}"), "HttpReads(UPDATE | SIGNAL_PAGES)");
        assert_eq!(format!("{:?}", HttpReads::EMPTY), "HttpReads()");
    }
}
