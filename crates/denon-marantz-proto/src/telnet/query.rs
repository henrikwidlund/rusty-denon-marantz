//! Queries that ask a receiver for its state.

use QueryGroup::{Core, Device, Names, Older, Picture, Signal, Sound, Speakers, Toggles, Zones};

use crate::brand::Brand;
use crate::sorted::{self, Keyed};
use crate::values::PictureMode;
use crate::zone::Zone;

/// Asks for the power state of the main zone.
pub const MAIN_ZONE_POWER_QUERY: &str = "ZM?";

/// Asks for the power state of the whole device.
pub const DEVICE_POWER_QUERY: &str = "PW?";

/// Defines [`Query`] and the list of all its values.
macro_rules! query_values {
    ($($(#[$doc:meta])* $name:ident,)*) => {
        /// A value that a receiver can be asked for. Each value has one query,
        /// or one for each brand. The answer comes as the events that a change
        /// of the value sends.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum Query {
            $($(#[$doc])* $name,)*
        }

        impl Query {
            /// All values.
            pub const ALL: &'static [Self] = &[$(Self::$name,)*];
        }
    };
}

query_values! {
        /// Selected input source.
        Source,
        /// Master volume.
        Volume,
        /// Mute of the main zone.
        Mute,
        /// Power of zone 2.
        Zone2Power,
        /// Mute of zone 2.
        Zone2Mute,
        /// Power of zone 3.
        Zone3Power,
        /// Mute of zone 3.
        Zone3Mute,
        /// Surround mode.
        SoundMode,
        /// On-screen menu.
        Menu,
        /// Quick select, or SMART select on a Marantz receiver.
        QuickSelect,
        /// Trigger outputs.
        Trigger,
        /// MDAX (Marantz).
        Mdax,
        /// DAC filter (Marantz).
        DacFilter,
        /// Illumination (Marantz).
        Illumination,
        /// Auto lip sync. Some Marantz receivers answer `SSALS` instead: ask
        /// for both.
        AutoLipSync,
        /// Auto lip sync (`SSALS`, Marantz), the other text for
        /// [`AutoLipSync`](Self::AutoLipSync).
        AutoLipSyncAls,
        /// Tone control on or off.
        ToneControl,
        /// Dynamic EQ.
        DynamicEq,
        /// Audyssey LFC.
        Lfc,
        /// Neural:X.
        NeuralX,
        /// IMAX audio settings, auto or manual.
        ImaxAudio,
        /// IMAX subwoofer mode.
        ImaxSubwooferMode,
        /// Subwoofer on or off.
        Subwoofer,
        /// Tactile transducer.
        TactileTransducer,
        /// HDMI audio decode.
        HdmiAudioDecode,
        /// Center spread.
        CenterSpread,
        /// Loudness management.
        LoudnessManagement,
        /// Cinema EQ.
        CinemaEq,
        /// Bluetooth transmitter.
        BluetoothTransmitter,
        /// Speaker virtualizer.
        SpeakerVirtualizer,
        /// Graphic EQ.
        GraphicEq,
        /// Headphone EQ.
        HeadphoneEq,
        /// Display dimmer.
        Dimmer,
        /// Eco mode.
        Eco,
        /// Sleep timer of the main zone.
        Sleep,
        /// Sleep timer of zone 2.
        Zone2Sleep,
        /// Sleep timer of zone 3.
        Zone3Sleep,
        /// Quick select of zone 2, or SMART select on a Marantz receiver.
        Zone2QuickSelect,
        /// Quick select of zone 3, or SMART select on a Marantz receiver.
        Zone3QuickSelect,
        /// HDMI monitor output.
        HdmiOutput,
        /// All zone stereo.
        AllZoneStereo,
        /// Audio delay (`PSDELAY`), the lip sync delay of the sound.
        AudioDelay,
        /// Video signal resolution.
        VideoResolution,
        /// Audio sampling rate.
        SamplingRate,
        /// Kind of the audio signal that comes in, for example Dolby or PCM.
        AudioSignal,
        /// Kind of the sound that goes out, after the sound mode.
        AudioSound,
        /// Output channels.
        OutputChannels,
        /// Input channels.
        InputChannels,
        /// Video signal input.
        VideoInput,
        /// Video signal output.
        VideoOutput,
        /// HDMI maximum resolution.
        HdmiMaxResolution,
        /// Video HDR format.
        HdrFormat,
        /// Video pixel depth.
        PixelDepth,
        /// Video maximum FRL.
        MaxFrl,
        /// Signal color space.
        ColorSpace,
        /// Signal extended colorimetry.
        ExtendedColorimetry,
        /// Dirac Live slots: name, filter and kind.
        DiracSlots,
        /// The selected Dirac Live filter.
        DiracFilter,
        /// Source names.
        SourceNames,
        /// Sources that are in use or hidden.
        SourcesInUse,
        /// Sound modes that the receiver offers.
        SoundModeList,
        /// Category of the sound mode: movie, music, game or pure.
        SoundModeCategory,
        /// Friendly name.
        FriendlyName,
        /// Quick select names.
        QuickSelectNames,
        /// Levels of the speaker channels, as in the setup menu (Denon).
        SpeakerLevels,
        /// Reference level offset.
        ReferenceLevel,
        /// Dynamic volume.
        DynamicVolume,
        /// Speaker preset.
        SpeakerPreset,
        /// Remote control lock.
        RemoteLock,
        /// Video select: on or off, and its source.
        VideoSelect,
        /// Picture mode.
        PictureMode,
        /// Digital codec.
        DigitalCodec,
        /// Bass.
        Bass,
        /// Treble.
        Treble,
        /// Containment amount.
        ContainmentAmount,
        /// MultEQ.
        MultEq,
        /// IMAX.
        Imax,
        /// IMAX high pass filter.
        ImaxHighPassFilter,
        /// IMAX low pass filter.
        ImaxLowPassFilter,
        /// IMAX subwoofer output.
        ImaxSubwooferOutput,
        /// Subwoofer level.
        SubwooferLevel,
        /// Auto standby of the main zone.
        AutoStandby,
        /// Auto standby of zone 2.
        Zone2AutoStandby,
        /// Auto standby of zone 3.
        Zone3AutoStandby,
        /// Video process.
        VideoProcess,
        /// Front speaker A or B.
        FrontSpeaker,
        /// Aspect ratio.
        AspectRatio,
        /// Vertical stretch.
        VerticalStretch,
        /// Analog to HDMI resolution.
        AnalogToHdmiResolution,
        /// HDMI upscaler.
        HdmiUpscaler,
        /// LFE level.
        LfeLevel,
        /// Bass sync.
        BassSync,
        /// Dialog enhancer.
        DialogEnhancer,
        /// Auro-Matic preset.
        AuroPreset,
        /// Auro-Matic strength.
        AuroStrength,
        /// Auro-3D mode.
        AuroMode,
        /// Room size.
        RoomSize,
        /// Dialog control.
        DialogControl,
        /// Effect speaker selection.
        EffectSpeakers,
        /// Front height output.
        FrontHeight,
        /// Dynamic range compression.
        Drc,
        /// Delay time (`PSDEL`) of the surround modes that have it.
        DelayTime,
        /// Audio restorer.
        AudioRestorer,
        /// Center level.
        CenterLevel,
        /// Panorama.
        Panorama,
        /// Surround level compensation.
        SurroundLevel,
        /// Dimension.
        Dimension,
        /// Center width (`PSCEN`). Some receivers answer `PSCEW` instead: ask
        /// for both.
        CenterWidth,
        /// Center width (`PSCEW`), the other text for [`CenterWidth`](Self::CenterWidth).
        CenterWidthCew,
        /// Effect.
        Effect,
        /// Audio input mode.
        AudioInputMode,
        /// Center image.
        CenterImage,
        /// Center gain.
        CenterGain,
        /// Channel volume of the main zone (`CV?`). Some receivers do not answer
        /// this query and send the list after an input change.
        ChannelVolume,
        /// Speaker configuration (Denon).
        SpeakerConfiguration,
        /// Speaker crossovers (Denon).
        SpeakerCrossovers,
        /// Graphic EQ of each speaker group (Denon).
        SpeakerGraphicEq,
        /// Volume settings of the main zone.
        MainZoneVolumeSettings,
        /// Volume settings of zone 2.
        Zone2VolumeSettings,
        /// Left and right level of zone 2 (`Z2CV?`).
        Zone2ChannelLevels,
        /// Left and right level of zone 3 (`Z3CV?`).
        Zone3ChannelLevels,
        /// Volume settings of zone 3.
        Zone3VolumeSettings,
        /// Bass of zone 2.
        Zone2Bass,
        /// Treble of zone 2.
        Zone2Treble,
        /// High pass filter of zone 2.
        Zone2HighPassFilter,
        /// Stereo or mono of zone 2.
        Zone2ChannelMode,
        /// HDMI audio of zone 2.
        Zone2HdmiAudio,
        /// Bass of zone 3.
        Zone3Bass,
        /// Treble of zone 3.
        Zone3Treble,
        /// High pass filter of zone 3.
        Zone3HighPassFilter,
        /// Stereo or mono of zone 3.
        Zone3ChannelMode,
        /// Bluetooth output level.
        BluetoothLevel,
        /// Channel and channel level of the tactile transducer (`CVTTR ?`).
        TactileChannel,
        /// Contrast.
        Contrast,
        /// Brightness.
        Brightness,
        /// Saturation.
        Saturation,
        /// Noise reduction.
        NoiseReduction,
        /// Enhancer.
        Enhancer,
        /// Pro Logic mode (`PSMODE`).
        ProLogicMode,
        /// Surround back mode (Denon).
        SurroundBackMode,
        /// Dynamic compression (`PSDCO`).
        DynamicCompression,
        /// Auto flag detect (Denon).
        AutoFlagDetect,
        /// Dialog level adjust and its level (Denon).
        DialogLevel,
        /// Audyssey DSX.
        AudysseyDsx,
        /// Stage width.
        StageWidth,
        /// Stage height.
        StageHeight,
        /// Height gain of Dolby `PLIIz`.
        HeightGain,
        /// `InstaPrevue`.
        InstaPrevue,
}

impl Query {
    /// The zone that the value belongs to. [`Zone::Main`] for values of the
    /// main zone and of the whole receiver.
    #[must_use]
    pub const fn zone(self) -> Zone {
        match self {
            Self::Zone2Power
            | Self::Zone2Mute
            | Self::Zone2Sleep
            | Self::Zone2QuickSelect
            | Self::Zone2AutoStandby
            | Self::Zone2VolumeSettings
            | Self::Zone2ChannelLevels
            | Self::Zone2Bass
            | Self::Zone2Treble
            | Self::Zone2HighPassFilter
            | Self::Zone2ChannelMode
            | Self::Zone2HdmiAudio => Zone::Zone2,
            Self::Zone3Power
            | Self::Zone3Mute
            | Self::Zone3Sleep
            | Self::Zone3QuickSelect
            | Self::Zone3AutoStandby
            | Self::Zone3VolumeSettings
            | Self::Zone3ChannelLevels
            | Self::Zone3Bass
            | Self::Zone3Treble
            | Self::Zone3HighPassFilter
            | Self::Zone3ChannelMode => Zone::Zone3,
            _ => Zone::Main,
        }
    }

    /// The part of the signal that a query of the [`QueryGroup::Signal`]
    /// group belongs to. `None` for other queries.
    #[must_use]
    pub fn signal_part(self) -> Option<SignalPart> {
        QUERIES
            .iter()
            .find(|entry| entry.query == self && entry.group == Signal)
            .map(|entry| SignalPart::of(entry.text))
    }

    /// The query text for this brand. `None` if the brand has no query for
    /// the value.
    #[must_use]
    pub fn text(self, brand: Brand) -> Option<&'static str> {
        self.entry(brand).map(|entry| entry.text)
    }

    /// The group of the query for this brand. `None` if the brand has no
    /// query for the value.
    #[must_use]
    pub fn group(self, brand: Brand) -> Option<QueryGroup> {
        self.entry(brand).map(|entry| entry.group)
    }

    /// The entry of [`QUERIES`] for this brand.
    fn entry(self, brand: Brand) -> Option<&'static Entry> {
        QUERIES
            .iter()
            .find(|entry| entry.query == self && entry.is_for(brand))
    }
}

/// The value that a query text of any brand asks for. A binary search, so
/// that filtering a whole list stays fast.
pub(crate) fn query_for_text(text: &str) -> Option<Query> {
    sorted::find(&BY_TEXT, (text, ""))
}

/// The query whose answer tells the value that a command line sets. `None`
/// if no query fits.
///
/// The query text without the `?` and the space before it must be the
/// longest start of the line. For example `Z2PSBAS 50` gives
/// [`Query::Zone2Bass`], `Z250` (zone 2 volume) gives [`Query::Zone2Power`]
/// and `PSDELAY 100` gives [`Query::AudioDelay`]. A `PV` line with a picture
/// mode code, such as `PVSTD`, gives [`Query::PictureMode`].
#[must_use]
pub fn query_for_command(line: &str) -> Option<Query> {
    // Some picture mode codes start like the text of another picture query,
    // for example `PVSTD` (Standard) like `PVST` (saturation).
    if line
        .strip_prefix("PV")
        .is_some_and(|code| PictureMode::from_telnet(code.trim_end()).is_some())
    {
        return Some(Query::PictureMode);
    }
    (1..=line.len().min(LONGEST_PREFIX))
        .rev()
        .filter_map(|len| line.get(..len))
        .find_map(|start| sorted::find(&BY_PREFIX, (start, "")))
}

/// The values of [`QUERIES`], sorted by text.
static BY_TEXT: [Keyed<Query>; QUERIES.len()] = sorted::sort(by_text());

const fn by_text() -> [Keyed<Query>; QUERIES.len()] {
    let mut list = [Keyed {
        key: ("", ""),
        value: Query::Source,
    }; QUERIES.len()];
    let mut i = 0;
    while i < list.len() {
        list[i] = Keyed {
            key: (QUERIES[i].text, ""),
            value: QUERIES[i].query,
        };
        i += 1;
    }
    list
}

/// The values of [`QUERIES`], sorted by [`command_start`].
static BY_PREFIX: [Keyed<Query>; QUERIES.len()] = sorted::sort(by_prefix());

/// The length of the longest key of [`BY_PREFIX`].
const LONGEST_PREFIX: usize = {
    let mut longest = 0;
    let mut i = 0;
    while i < QUERIES.len() {
        let len = command_start(QUERIES[i].text).len();
        if len > longest {
            longest = len;
        }
        i += 1;
    }
    longest
};

const fn by_prefix() -> [Keyed<Query>; QUERIES.len()] {
    let mut list = by_text();
    let mut i = 0;
    while i < list.len() {
        list[i].key.0 = command_start(list[i].key.0);
        i += 1;
    }
    list
}

/// The query text without the `?` at the end and the spaces before it: the
/// start of the commands that set the value.
const fn command_start(text: &'static str) -> &'static str {
    let bytes = match text.as_bytes().split_last() {
        Some((b'?', rest)) => rest.trim_ascii_end(),
        _ => text.as_bytes(),
    };
    match core::str::from_utf8(bytes) {
        Ok(start) => start,
        Err(_) => panic!("query text is not ASCII"),
    }
}

/// Brands that a query is for.
#[derive(Debug, Clone, Copy)]
enum Brands {
    Both,
    Denon,
    Marantz,
}

#[derive(Debug, Clone, Copy)]
struct Entry {
    query: Query,
    text: &'static str,
    brands: Brands,
    group: QueryGroup,
}

impl Entry {
    const fn is_for(&self, brand: Brand) -> bool {
        matches!(
            (self.brands, brand),
            (Brands::Both, _) | (Brands::Denon, Brand::Denon) | (Brands::Marantz, Brand::Marantz)
        )
    }
}

const fn both(group: QueryGroup, query: Query, text: &'static str) -> Entry {
    Entry {
        query,
        text,
        brands: Brands::Both,
        group,
    }
}

const fn denon(group: QueryGroup, query: Query, text: &'static str) -> Entry {
    Entry {
        query,
        text,
        brands: Brands::Denon,
        group,
    }
}

const fn marantz(group: QueryGroup, query: Query, text: &'static str) -> Entry {
    Entry {
        query,
        text,
        brands: Brands::Marantz,
        group,
    }
}

labeled_enum! {
    /// A group of queries that ask for related values. A client chooses which
    /// groups the connection fetches and in which order, and can fetch a group
    /// later.
    QueryGroup {
        Core = "Core";
        Names = "Names";
        Signal = "Signal";
        Toggles = "Toggles";
        Device = "Device";
        Sound = "Sound";
        Zones = "Zones";
        Picture = "Picture";
        Speakers = "Speakers";
        Older = "Older";
    }
}

impl QueryGroup {
    /// All groups, the most important first. Fetch them all with
    /// `TelnetConfig::query_groups` or one by one when they are needed.
    ///
    /// 1. [`Core`](Self::Core): power, source, volume and mute of the zones,
    ///    and the sound mode.
    /// 2. [`Names`](Self::Names): source names, sound mode list, friendly name,
    ///    Dirac slots and the selected Dirac filter. The state needs the
    ///    filter to know which commands the receiver ignores.
    /// 3. [`Toggles`](Self::Toggles): on and off settings that clients toggle.
    ///    A toggle needs the current value.
    /// 4. [`Signal`](Self::Signal): video and audio signal information, which
    ///    clients show.
    /// 5. [`Device`](Self::Device): dimmer, eco, sleep, standby, remote lock,
    ///    video select, video and picture settings, Bluetooth level.
    /// 6. [`Sound`](Self::Sound): the sound settings and levels.
    /// 7. [`Zones`](Self::Zones): the volume settings, levels, tone, high
    ///    pass filter and channel mode of the zones, and the HDMI audio of
    ///    zone 2.
    /// 8. [`Picture`](Self::Picture): the picture mode and the picture
    ///    adjustments. Only older receivers answer these.
    /// 9. [`Speakers`](Self::Speakers): the speaker configuration, the
    ///    crossovers and the graphic EQ. Only Denon receivers answer these.
    /// 10. [`Older`](Self::Older): settings of receivers from about 2011 to
    ///     2015. The command lists of receivers with HEOS do not have these.
    ///
    /// These lists are not filtered. The connection of the `denon-marantz`
    /// crate leaves out queries that the
    /// [`SpecGeneration`](crate::SpecGeneration) of the receiver does not
    /// have, in any group. Filter with
    /// [`SpecGeneration::allows`](crate::SpecGeneration::allows).
    pub const DEFAULT_ORDER: &[Self] = &[
        Self::Core,
        Self::Names,
        Self::Toggles,
        Self::Signal,
        Self::Device,
        Self::Sound,
        Self::Zones,
        Self::Picture,
        Self::Speakers,
        Self::Older,
    ];

    /// The groups that a connection fetches by default: the ones that most
    /// clients show or toggle at once. The others stay empty until the
    /// receiver reports a change, or until a client fetches them.
    pub const START_ORDER: &[Self] = &[Self::Core, Self::Names, Self::Toggles, Self::Signal];
}

/// Queries of the chosen groups, in the order of `groups`. A group that is
/// listed twice counts once. The receiver answers each query with the same
/// events as for changes.
pub fn queries(
    brand: Brand,
    groups: &'static [QueryGroup],
) -> impl Iterator<Item = &'static str> + Clone {
    groups
        .iter()
        .enumerate()
        .filter(|(index, group)| !groups[..*index].contains(group))
        .flat_map(move |(_, group)| group_queries(brand, *group))
}

/// Queries of one group, in the order that they are sent.
pub fn group_queries(
    brand: Brand,
    group: QueryGroup,
) -> impl Iterator<Item = &'static str> + Clone {
    QUERIES
        .iter()
        .filter(move |query| query.group == group && query.is_for(brand))
        .map(|query| query.text)
}

/// Which read answers a query of the [`QueryGroup::Signal`] group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SignalPart {
    /// The video signal. The web interface page
    /// [`VIDEO_SIGNAL_PATH`](crate::VIDEO_SIGNAL_PATH) answers it.
    Video,
    /// The channels. The web interface page
    /// [`AUDIO_CHANNELS_PATH`](crate::AUDIO_CHANNELS_PATH) answers it.
    Channels,
    /// The kind of the audio signal and of the sound, and the sampling rate.
    /// `AppCommand0300.xml` answers it too ([`GET_AUDIO_INFO`](crate::appcommand::GET_AUDIO_INFO)).
    AudioInfo,
    /// The HDMI limit. Only telnet answers it.
    TelnetOnly,
    /// The extended colorimetry (BT.2020). Telnet answers it. The video page
    /// of the web interface has it only for some signals. Ask it after the
    /// video queries, so that its answer comes after theirs.
    Colorimetry,
}

impl SignalPart {
    fn of(text: &str) -> Self {
        match text {
            "SSINFSIGRES ?" | "SYSDVIN ?" | "SYSDVOUT ?" | "SSINFSIGHDR ?" | "SSINFSIGPIX ?"
            | "SSINFSIGFRL ?" | "SSINFSIGCOS ?" => Self::Video,
            "OPINFASP ?" | "OPINFINS ?" => Self::Channels,
            "SSINFAISFSV ?" | "SYSDA ?" | "SYSMI ?" => Self::AudioInfo,
            "SSINFSIGECI ?" => Self::Colorimetry,
            _ => Self::TelnetOnly,
        }
    }
}

/// The queries of the signal group that belong to one part, in the order of
/// the group.
pub fn signal_queries(
    brand: Brand,
    part: SignalPart,
) -> impl Iterator<Item = &'static str> + Clone {
    group_queries(brand, Signal).filter(move |text| SignalPart::of(text) == part)
}

/// The queries to send first when the state was read with `AppCommand`.
///
/// `AppCommand` does not answer these, and clients need them soon: the quick
/// select (or the SMART select), the toggles, the sound modes that the
/// receiver offers and the Dirac Live slots.
pub fn after_appcommand_queries(brand: Brand) -> impl Iterator<Item = &'static str> + Clone {
    let only = |texts: &'static [&'static str]| {
        QUERIES
            .iter()
            .filter(move |query| query.is_for(brand) && texts.contains(&query.text))
            .map(|query| query.text)
    };
    only(&["MSSMART ?", "MSQUICK ?"])
        .chain(group_queries(brand, Toggles))
        .chain(only(&["OPDLS ?", "PSDIRAC ?", "OPSMLALL ?"]))
}

/// Queries to send after connect with the default order of the groups.
///
/// Brand-specific queries are in the place where they are sent.
pub fn state_queries(brand: Brand) -> impl Iterator<Item = &'static str> + Clone {
    queries(brand, QueryGroup::DEFAULT_ORDER)
}

/// Queries whose answer is a list that ends with an `END` line, for example
/// `OPSMLALL END` or `CVEND`. Only lists whose end line was seen on a receiver
/// or is in a spec. `Z2CV?` and `Z3CV?` have no end line.
const LIST_QUERIES: [&str; 10] = [
    "OPSMLALL ?",
    "OPDLS ?",
    "SSFUN ?",
    "SSSOD ?",
    "SSTTR ?",
    "SSCFR ?",
    "SSAEQLRS ?",
    "SSLEV ?",
    "SSQSNZMA ?",
    "CV?",
];

/// True if the answer to the query is a list that ends with an `END` line.
pub(super) fn is_list_query(query: &str) -> bool {
    LIST_QUERIES.contains(&query)
}

/// The texts that the answers to the queries of this brand start with.
pub(super) fn known_answer_prefixes(brand: Brand) -> impl Iterator<Item = &'static str> {
    QUERIES
        .iter()
        .filter(move |query| query.is_for(brand))
        .map(|query| super::answers::answer_prefix(query.text))
}

static QUERIES: [Entry; 153] = [
    both(Core, Query::Source, "SI?"),      // Select INPUT source
    both(Core, Query::Volume, "MV?"),      // MASTER VOLUME
    both(Core, Query::Mute, "MU?"),        // Mute
    both(Core, Query::Zone2Power, "Z2?"),  // Z2 Zone Power
    both(Core, Query::Zone2Mute, "Z2MU?"), // Z2 Mute
    both(Core, Query::Zone3Power, "Z3?"),  // Z3 Zone Power
    both(Core, Query::Zone3Mute, "Z3MU?"), // Z3 Mute
    both(Core, Query::SoundMode, "MS?"),   // Surround mode
    both(Toggles, Query::Menu, "MNMEN?"),  // Menu
    marantz(Core, Query::QuickSelect, "MSSMART ?"), // SMART Select
    denon(Core, Query::QuickSelect, "MSQUICK ?"), // Quick Select
    both(Toggles, Query::Trigger, "TR?"),  // Trigger
    marantz(Toggles, Query::Mdax, "PSMDAX ?"), // MDAX
    marantz(Toggles, Query::DacFilter, "PSDACFIL ?"), // DAC Filter
    marantz(Toggles, Query::Illumination, "ILB ?"), // Illumination
    marantz(Toggles, Query::AutoLipSync, "SSHOS ?"), // Auto Lip Sync
    marantz(Toggles, Query::AutoLipSyncAls, "SSALS ?"), // Auto Lip Sync, current receivers
    both(Toggles, Query::ToneControl, "PSTONE CTRL ?"), // TONE
    both(Toggles, Query::DynamicEq, "PSDYNEQ ?"), // Dynamic EQ
    both(Toggles, Query::Lfc, "PSLFC ?"),  // Audyssey LFC
    both(Toggles, Query::NeuralX, "PSNEURAL ?"), // Neural:X
    both(Toggles, Query::ImaxAudio, "PSIMAXAUD ?"), // IMAX Audio Settings Auto/Manual
    both(Toggles, Query::ImaxSubwooferMode, "PSIMAXSWM ?"), // IMAX Subwoofer Mode
    both(Toggles, Query::Subwoofer, "PSSWR ?"), // Subwoofer
    both(Toggles, Query::TactileTransducer, "SSTTR ?"), // Tactile Transducer
    both(Toggles, Query::HdmiAudioDecode, "VSAUDIO ?"), // HDMI Audio Decode
    both(Toggles, Query::CenterSpread, "PSCES ?"), // CENTER Spread
    both(Toggles, Query::LoudnessManagement, "PSLOM ?"), // Loudness Management
    both(Toggles, Query::CinemaEq, "PSCINEMA EQ. ?"), // CINEMA EQ
    both(Toggles, Query::BluetoothTransmitter, "BTTX ?"), // Bluetooth Transmitter
    both(Toggles, Query::SpeakerVirtualizer, "PSSPV ?"), // Speaker Virtualizer
    both(Toggles, Query::GraphicEq, "PSGEQ ?"), // Graphic EQ
    both(Toggles, Query::HeadphoneEq, "PSHEQ ?"), // Headphone EQ
    both(Device, Query::Dimmer, "DIM ?"),  // Dimmer
    both(Device, Query::Eco, "ECO?"),      // ECO
    both(Device, Query::Sleep, "SLP?"),    // Sleep
    both(Device, Query::Zone2Sleep, "Z2SLP?"), // Z2 Sleep
    both(Device, Query::Zone3Sleep, "Z3SLP?"), // Z3 Sleep
    denon(Device, Query::Zone2QuickSelect, "Z2QUICK ?"), // Z2 Quick Select
    denon(Device, Query::Zone3QuickSelect, "Z3QUICK ?"), // Z3 Quick Select
    marantz(Device, Query::Zone2QuickSelect, "Z2SMART ?"), // Z2 SMART Select
    marantz(Device, Query::Zone3QuickSelect, "Z3SMART ?"), // Z3 SMART Select
    both(Device, Query::HdmiOutput, "VSMONI ?"), // HDMI Output
    both(Device, Query::RemoteLock, "SYREMOTE LOCK ?"), // Remote lock
    both(Device, Query::VideoSelect, "SV?"), // Video Select
    both(Device, Query::BluetoothLevel, "BTLEV ?"), // Bluetooth Level
    both(Sound, Query::AllZoneStereo, "MNZST?"), // All zone stereo
    both(Sound, Query::AudioDelay, "PSDELAY ?"), // Audio Delay
    both(Signal, Query::VideoResolution, "SSINFSIGRES ?"), // Video Signal
    both(Signal, Query::SamplingRate, "SSINFAISFSV ?"), // Audio Sampling Rate
    both(Signal, Query::AudioSignal, "SYSDA ?"), // Audio Signal
    both(Signal, Query::AudioSound, "SYSMI ?"), // Audio Sound
    both(Signal, Query::OutputChannels, "OPINFASP ?"), // Output Channels
    both(Signal, Query::InputChannels, "OPINFINS ?"), // Input Channels
    both(Signal, Query::VideoInput, "SYSDVIN ?"), // Video Signal Input
    both(Signal, Query::VideoOutput, "SYSDVOUT ?"), // Video Signal Output
    both(Signal, Query::HdmiMaxResolution, "SYHDMIDIAGMAXRES ?"), // HDMI Max Resolution
    both(Signal, Query::HdrFormat, "SSINFSIGHDR ?"), // Video HDR Format
    both(Signal, Query::PixelDepth, "SSINFSIGPIX ?"), // Video Pixel Depth
    both(Signal, Query::MaxFrl, "SSINFSIGFRL ?"), // Video Max FRL
    both(Signal, Query::ColorSpace, "SSINFSIGCOS ?"), // Signal Color Space
    both(Signal, Query::ExtendedColorimetry, "SSINFSIGECI ?"), // Signal Extended Colorimetry
    both(Names, Query::DiracSlots, "OPDLS ?"), // Dirac Live slots: name, filter and kind
    both(Names, Query::DiracFilter, "PSDIRAC ?"), // Dirac Live filter that is selected
    both(Names, Query::SourceNames, "SSFUN ?"), // Source names
    both(Names, Query::SourcesInUse, "SSSOD ?"), // Sources in use or hidden
    both(Names, Query::SoundModeList, "OPSMLALL ?"), // Sound modes that the receiver offers
    both(Names, Query::FriendlyName, "NSFRN ?"), // Friendly name. "NSFRN?" breaks HEOS
    denon(Toggles, Query::AutoLipSync, "OPALS ?"), // Auto Lip Sync
    denon(Names, Query::QuickSelectNames, "SSQS ?"), // Quick select names
    marantz(Names, Query::QuickSelectNames, "SSQSNZMA ?"), // Quick select names
    denon(Sound, Query::SpeakerLevels, "SSLEV ?"), // Speaker channel levels
    both(Sound, Query::ReferenceLevel, "PSREFLEV ?"), // Reference Level
    both(Sound, Query::DynamicVolume, "PSDYNVOL ?"), // Dynamic Vol.
    both(Device, Query::SpeakerPreset, "SPPR ?"), // Speaker Preset
    both(Picture, Query::PictureMode, "PV?"), // Picture Mode
    both(Device, Query::DigitalCodec, "DC?"), // Digital Codec
    both(Sound, Query::Bass, "PSBAS ?"),   // BASS
    both(Sound, Query::Treble, "PSTRE ?"), // TREBLE
    both(Sound, Query::ContainmentAmount, "PSCNTAMT ?"), // Containment Amount
    both(Sound, Query::MultEq, "PSMULTEQ: ?"), // MultEQ
    both(Sound, Query::Imax, "PSIMAX ?"),  // IMAX
    both(Sound, Query::ImaxHighPassFilter, "PSIMAXHPF ?"), // IMAX High Pass Filter
    both(Sound, Query::ImaxLowPassFilter, "PSIMAXLPF ?"), // IMAX Low Pass Filter
    both(Sound, Query::ImaxSubwooferOutput, "PSIMAXSWO ?"), // Subwoofer Output LFE+Main/LFE
    both(Sound, Query::SubwooferLevel, "PSSWL ?"), // Subwoofer Level
    both(Device, Query::AutoStandby, "STBY?"), // Auto Standby
    both(Device, Query::Zone2AutoStandby, "Z2STBY?"), // Auto Standby
    both(Device, Query::Zone3AutoStandby, "Z3STBY?"), // Auto Standby
    both(Device, Query::VideoProcess, "VSVPM ?"), // Video Process
    both(Device, Query::FrontSpeaker, "PSFRONT?"), // Front Speaker A/B
    both(Device, Query::AspectRatio, "VSASP ?"), // Aspect Ratio
    both(Device, Query::VerticalStretch, "VSVST ?"), // Vertical Stretch
    both(Device, Query::AnalogToHdmiResolution, "VSSC ?"), // Analog to HDMI Resolution
    both(Device, Query::HdmiUpscaler, "VSSCH ?"), // HDMI Upscaler
    both(Sound, Query::LfeLevel, "PSLFE ?"), // LFE Level
    both(Sound, Query::BassSync, "PSBSC ?"), // Bass Sync
    both(Sound, Query::DialogEnhancer, "PSDEH ?"), // Dialog Enhancer
    both(Sound, Query::AuroPreset, "PSAUROPR ?"), // Auro-Matic Preset
    both(Sound, Query::AuroStrength, "PSAUROST ?"), // Auro-Matic Strength
    both(Sound, Query::AuroMode, "PSAUROMODE ?"), // AURO-3D Mode
    both(Sound, Query::RoomSize, "PSRSZ ?"), // ROOM SIZE
    both(Sound, Query::DialogControl, "PSDIC ?"), // Dialog Control
    both(Sound, Query::EffectSpeakers, "PSSP: ?"), // Effect Speaker selection
    both(Sound, Query::FrontHeight, "PSFH: ?"), // Front height output
    both(Sound, Query::Drc, "PSDRC ?"),    // DRC
    both(Sound, Query::DelayTime, "PSDEL ?"), // DELAY TIME
    both(Sound, Query::SoundModeCategory, "SSSMG ?"), // Sound mode category
    both(Sound, Query::AudioRestorer, "PSRSTR ?"), // AUDIO RESTORER
    both(Sound, Query::CenterLevel, "PSCLV ?"), // Center level
    both(Sound, Query::Panorama, "PSPAN ?"), // Panorama
    both(Sound, Query::SurroundLevel, "PSSURLEV ?"), // Surround level compensation
    both(Sound, Query::Dimension, "PSDIM ?"), // Dimension
    both(Sound, Query::CenterWidth, "PSCEN ?"), // Center width
    both(Sound, Query::CenterWidthCew, "PSCEW ?"), // Center width, current receivers
    both(Sound, Query::Effect, "PSEFF ?"), // Effect
    both(Sound, Query::AudioInputMode, "SD?"), // Audio input mode
    both(Sound, Query::CenterImage, "PSCEI ?"), // Center image
    both(Sound, Query::CenterGain, "PSCEG ?"), // Center gain
    both(Sound, Query::TactileChannel, "CVTTR ?"), // Tactile Transducer channel
    // The A10H and its app never get an answer to this query. It pushes the list after an input change.
    both(Sound, Query::ChannelVolume, "CV?"), // Channel Volume
    denon(Speakers, Query::SpeakerConfiguration, "SSSPC ?"), // Speaker configuration
    denon(Speakers, Query::SpeakerCrossovers, "SSCFR ?"), // Speaker crossovers
    denon(Speakers, Query::SpeakerGraphicEq, "SSAEQLRS ?"), // Graphic EQ of each speaker group
    both(Zones, Query::MainZoneVolumeSettings, "SSVCTZMA ?"), // Volume settings of the main zone
    both(Zones, Query::Zone2VolumeSettings, "SSVCTZ2S ?"), // Volume settings of zone 2
    both(Zones, Query::Zone2ChannelLevels, "Z2CV?"), // Channel levels of zone 2
    both(Zones, Query::Zone3ChannelLevels, "Z3CV?"), // Channel levels of zone 3
    both(Zones, Query::Zone3VolumeSettings, "SSVCTZ3S ?"), // Volume settings of zone 3
    both(Zones, Query::Zone2Bass, "Z2PSBAS ?"), // Z2 Bass
    both(Zones, Query::Zone2Treble, "Z2PSTRE ?"), // Z2 Treble
    both(Zones, Query::Zone2HighPassFilter, "Z2HPF?"), // Z2 HPF
    both(Zones, Query::Zone2ChannelMode, "Z2CS?"), // Z2 Channel Setting
    both(Zones, Query::Zone2HdmiAudio, "Z2HDA?"), // Z2 HDMI Audio
    both(Zones, Query::Zone3Bass, "Z3PSBAS ?"), // Z3 Bass
    both(Zones, Query::Zone3Treble, "Z3PSTRE ?"), // Z3 Treble
    both(Zones, Query::Zone3HighPassFilter, "Z3HPF?"), // Z3 HPF
    both(Zones, Query::Zone3ChannelMode, "Z3CS?"), // Z3 Channel Setting
    both(Picture, Query::Contrast, "PVCN ?"), // Contrast
    both(Picture, Query::Brightness, "PVBR ?"), // Brightness
    both(Picture, Query::Saturation, "PVST ?"), // Saturation
    both(Picture, Query::NoiseReduction, "PVDNR ?"), // Noise Reduction
    both(Picture, Query::Enhancer, "PVENH ?"), // Enhancer
    both(Older, Query::ProLogicMode, "PSMODE: ?"), // Pro Logic mode
    denon(Older, Query::SurroundBackMode, "PSSB: ?"), // Surround back mode
    both(Older, Query::DynamicCompression, "PSDCO ?"), // Dynamic compression
    denon(Older, Query::AutoFlagDetect, "PSAFD ?"), // Auto flag detect
    denon(Older, Query::DialogLevel, "PSDIL ?"), // Dialog level adjust and level
    both(Older, Query::AudysseyDsx, "PSDSX ?"), // Audyssey DSX
    both(Older, Query::StageWidth, "PSSTW ?"), // Stage width
    both(Older, Query::StageHeight, "PSSTH ?"), // Stage height
    both(Older, Query::HeightGain, "PSPHG ?"), // PLIIz height gain
    both(Older, Query::InstaPrevue, "MNPRV?"), // InstaPrevue
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telnet::Command;

    const BRANDS: [Brand; 2] = [Brand::Denon, Brand::Marantz];

    #[test]
    fn commands_find_the_query_with_the_longest_start() {
        assert!(sorted::is_strictly_sorted(&BY_PREFIX));
        for (line, query) in [
            ("Z2PSBAS 50", Some(Query::Zone2Bass)),
            ("Z2MU ON", Some(Query::Zone2Mute)),
            ("Z250", Some(Query::Zone2Power)),
            ("Z3MU ON", Some(Query::Zone3Mute)),
            ("Z3PSTRE 50", Some(Query::Zone3Treble)),
            ("Z3ON", Some(Query::Zone3Power)),
            ("PSDELAY 100", Some(Query::AudioDelay)),
            ("PSDEL 100", Some(Query::DelayTime)),
            ("BTLEV UP", Some(Query::BluetoothLevel)),
            ("BTTX ON", Some(Query::BluetoothTransmitter)),
            ("PSSPV ON", Some(Query::SpeakerVirtualizer)),
            ("PSSP:FH", Some(Query::EffectSpeakers)),
            ("PSTONE CTRL ON", Some(Query::ToneControl)),
            ("PSCINEMA EQ.ON", Some(Query::CinemaEq)),
            ("MSQUICK1", Some(Query::QuickSelect)),
            ("MSDOLBY DIGITAL", Some(Query::SoundMode)),
            ("MSSMART1", Some(Query::QuickSelect)),
            ("Z2SMART1", Some(Query::Zone2QuickSelect)),
            // A `PV` setter without its own query needs the picture feature.
            ("PVHUE 50", Some(Query::PictureMode)),
            ("PVSTD", Some(Query::PictureMode)),
            ("PVSTM\r", Some(Query::PictureMode)),
            ("PVBRL", Some(Query::PictureMode)),
            ("PVCSTM", Some(Query::PictureMode)),
            ("PVMOV", Some(Query::PictureMode)),
            ("PVST 50", Some(Query::Saturation)),
            ("PVBR 50", Some(Query::Brightness)),
            ("PVSTX", Some(Query::Saturation)),
            ("CVTTR UP", Some(Query::TactileChannel)),
            ("VSSCHAUTO", Some(Query::HdmiUpscaler)),
            ("VSSC48P", Some(Query::AnalogToHdmiResolution)),
            ("PSIMAXAUD AUTO", Some(Query::ImaxAudio)),
            ("SYHDMIDIAGMAXRES X", Some(Query::HdmiMaxResolution)),
            ("PSLFL 05", None),
            ("NS9A", None),
            ("", None),
            ("Zé", None),
        ] {
            assert_eq!(query_for_command(line), query, "{line}");
        }
    }

    #[test]
    #[expect(clippy::too_many_lines, reason = "one table of cases")]
    fn typed_commands_find_their_query() {
        use crate::{DeviceSettingValue, SoundSettingValue as S, ToneLevel, ZoneSettingValue as Z};
        let tone = ToneLevel::from_db(0).unwrap();
        let cases = [
            (Command::Mute(true), Zone::Zone2, Query::Zone2Mute),
            (Command::Mute(true), Zone::Zone3, Query::Zone3Mute),
            (Command::Mute(true), Zone::Main, Query::Mute),
            (
                Command::SetZoneSetting(Z::Bass(tone)),
                Zone::Zone2,
                Query::Zone2Bass,
            ),
            (
                Command::SetZoneSetting(Z::Treble(tone)),
                Zone::Zone3,
                Query::Zone3Treble,
            ),
            (
                Command::SetZoneSetting(Z::HighPassFilter(true)),
                Zone::Zone3,
                Query::Zone3HighPassFilter,
            ),
            (
                Command::SetZoneSetting(Z::QuickSelect(1)),
                Zone::Main,
                Query::QuickSelect,
            ),
            (
                Command::SetZoneSetting(Z::QuickSelect(1)),
                Zone::Zone2,
                Query::Zone2QuickSelect,
            ),
            (Command::SetBass(tone), Zone::Main, Query::Bass),
            (Command::ToneControl(true), Zone::Main, Query::ToneControl),
            (Command::DynamicEq(true), Zone::Main, Query::DynamicEq),
            (Command::Lfc(true), Zone::Main, Query::Lfc),
            (Command::InstaPrevue(true), Zone::Main, Query::InstaPrevue),
            (
                Command::AllZoneStereo(true),
                Zone::Main,
                Query::AllZoneStereo,
            ),
            (
                Command::SetSoundSetting(S::LfeLevel(5)),
                Zone::Main,
                Query::LfeLevel,
            ),
            (
                Command::SetSoundSetting(S::ImaxSubwooferMode(true)),
                Zone::Main,
                Query::ImaxSubwooferMode,
            ),
            (
                Command::SetSoundSetting(S::ImaxHighPass(40)),
                Zone::Main,
                Query::ImaxHighPassFilter,
            ),
            (
                Command::SetSoundSetting(S::CenterSpread(true)),
                Zone::Main,
                Query::CenterSpread,
            ),
            (
                Command::SetSoundSetting(S::Subwoofer(true)),
                Zone::Main,
                Query::Subwoofer,
            ),
            (
                Command::SetSoundSetting(S::SpeakerVirtualizer(true)),
                Zone::Main,
                Query::SpeakerVirtualizer,
            ),
            (
                Command::SetSoundSetting(S::GraphicEq(true)),
                Zone::Main,
                Query::GraphicEq,
            ),
            (
                Command::SetSoundSetting(S::HeadphoneEq(true)),
                Zone::Main,
                Query::HeadphoneEq,
            ),
            (
                Command::SetSoundSetting(S::LoudnessManagement(true)),
                Zone::Main,
                Query::LoudnessManagement,
            ),
            (
                Command::SetSoundSetting(S::Effect(true)),
                Zone::Main,
                Query::Effect,
            ),
            (
                Command::SetSoundSetting(S::CenterImage(3)),
                Zone::Main,
                Query::CenterImage,
            ),
            (
                Command::SetSoundSetting(S::AuroMaticStrength(5)),
                Zone::Main,
                Query::AuroStrength,
            ),
            (
                Command::SetDeviceSetting(DeviceSettingValue::BluetoothLevel(50)),
                Zone::Main,
                Query::BluetoothLevel,
            ),
            (
                Command::SetDeviceSetting(DeviceSettingValue::BluetoothTransmitter(true)),
                Zone::Main,
                Query::BluetoothTransmitter,
            ),
            (
                Command::SetDeviceSetting(DeviceSettingValue::RemoteLock(true)),
                Zone::Main,
                Query::RemoteLock,
            ),
        ];
        for brand in BRANDS {
            for (command, zone, query) in &cases {
                let mut line = String::new();
                command.write_line(brand, *zone, &mut line).unwrap();
                assert_eq!(query_for_command(&line), Some(*query), "{brand:?} {line:?}");
            }
        }
        for mode in [
            PictureMode::Standard,
            PictureMode::Stream,
            PictureMode::Brilliant,
        ] {
            let command = Command::SetPictureSetting(crate::PictureSettingValue::Mode(mode));
            let mut line = String::new();
            command
                .write_line(Brand::Denon, Zone::Main, &mut line)
                .unwrap();
            assert_eq!(
                query_for_command(&line),
                Some(Query::PictureMode),
                "{line:?}"
            );
        }
        let lfe_input = Command::SetSoundSetting(S::LfeExternalInput(5));
        let mut line = String::new();
        lfe_input
            .write_line(Brand::Denon, Zone::Main, &mut line)
            .unwrap();
        assert_eq!(query_for_command(&line), None, "{line:?}");
    }

    #[test]
    fn spec_queries_are_in_groups_that_are_not_sent_at_the_start() {
        for entry in &QUERIES {
            if entry.query.spec_feature().is_some() {
                assert!(
                    !QueryGroup::START_ORDER.contains(&entry.group),
                    "{}",
                    entry.text
                );
            }
        }
    }

    #[test]
    fn current_receivers_get_no_older_query() {
        for brand in BRANDS {
            for query in group_queries(brand, Older) {
                assert!(!crate::SpecGeneration::Current.allows(query), "{query}");
            }
        }
    }

    #[test]
    fn signal_part_follows_the_group() {
        assert_eq!(Query::HdrFormat.signal_part(), Some(SignalPart::Video));
        assert_eq!(
            Query::InputChannels.signal_part(),
            Some(SignalPart::Channels)
        );
        assert_eq!(
            Query::SamplingRate.signal_part(),
            Some(SignalPart::AudioInfo)
        );
        assert_eq!(
            Query::ExtendedColorimetry.signal_part(),
            Some(SignalPart::Colorimetry)
        );
        assert_eq!(Query::Volume.signal_part(), None);
    }

    #[test]
    fn zone_follows_the_name() {
        for query in Query::ALL {
            let name = format!("{query:?}");
            let zone = if name.starts_with("Zone2") {
                Zone::Zone2
            } else if name.starts_with("Zone3") {
                Zone::Zone3
            } else {
                Zone::Main
            };
            assert_eq!(query.zone(), zone, "{name}");
        }
    }

    #[test]
    fn every_list_query_is_a_known_query() {
        for list in LIST_QUERIES {
            assert!(QUERIES.iter().any(|entry| entry.text == list), "{list}");
        }
    }

    #[test]
    fn every_query_asks_for_a_value() {
        for brand in BRANDS {
            for query in state_queries(brand) {
                assert!(query.ends_with('?'), "{query}");
            }
        }
    }

    #[test]
    fn every_value_has_one_query_for_each_brand_that_asks_it() {
        for query in Query::ALL {
            let denon = query.text(Brand::Denon);
            let marantz = query.text(Brand::Marantz);
            assert!(denon.is_some() || marantz.is_some(), "{query:?}");
            for brand in BRANDS {
                let rows = QUERIES
                    .iter()
                    .filter(|entry| entry.query == *query && entry.is_for(brand))
                    .count();
                assert!(rows <= 1, "{query:?} has {rows} queries for {brand:?}");
            }
        }
    }

    #[test]
    fn a_value_gives_the_query_of_the_brand() {
        assert_eq!(Query::DiracFilter.text(Brand::Denon), Some("PSDIRAC ?"));
        assert_eq!(Query::QuickSelect.text(Brand::Denon), Some("MSQUICK ?"));
        assert_eq!(Query::QuickSelect.text(Brand::Marantz), Some("MSSMART ?"));
        assert_eq!(Query::AutoLipSync.text(Brand::Denon), Some("OPALS ?"));
        assert_eq!(Query::AutoLipSync.text(Brand::Marantz), Some("SSHOS ?"));
        assert_eq!(Query::AutoLipSyncAls.text(Brand::Marantz), Some("SSALS ?"));
        assert_eq!(Query::AutoLipSyncAls.text(Brand::Denon), None);
        assert_eq!(Query::SpeakerGraphicEq.text(Brand::Marantz), None);
    }

    #[test]
    fn the_values_of_a_brand_are_the_queries_of_the_brand() {
        for brand in BRANDS {
            let mut from_values: Vec<_> = Query::ALL
                .iter()
                .filter_map(|query| query.text(brand))
                .collect();
            let mut asked: Vec<_> = state_queries(brand).collect();
            from_values.sort_unstable();
            asked.sort_unstable();
            assert_eq!(from_values, asked);
        }
    }

    #[test]
    fn speaker_queries_come_before_the_older_ones_and_are_for_denon() {
        let denon: Vec<_> = state_queries(Brand::Denon).collect();
        let marantz: Vec<_> = state_queries(Brand::Marantz).collect();
        let end = denon.len() - group_queries(Brand::Denon, Older).count();
        assert_eq!(denon[end - 3..end], ["SSSPC ?", "SSCFR ?", "SSAEQLRS ?"]);
        assert!(!marantz.contains(&"SSAEQLRS ?"));
        assert!(denon.contains(&"SD?") && marantz.contains(&"SD?"));
        assert!(QueryGroup::DEFAULT_ORDER.ends_with(&[Speakers, Older]));
    }

    #[test]
    fn each_query_text_is_found() {
        for entry in &QUERIES {
            assert_eq!(
                query_for_text(entry.text),
                Some(entry.query),
                "{}",
                entry.text
            );
        }
        assert_eq!(query_for_text("XYZ ?"), None);
        assert_eq!(query_for_text(""), None);
        // Each text is listed once.
        assert!(sorted::is_strictly_sorted(&BY_TEXT));
    }

    #[test]
    fn brand_specific_queries() {
        let denon: Vec<_> = state_queries(Brand::Denon).collect();
        let marantz: Vec<_> = state_queries(Brand::Marantz).collect();
        assert!(denon.contains(&"MSQUICK ?"));
        assert!(!denon.contains(&"MSSMART ?"));
        assert!(marantz.contains(&"MSSMART ?"));
        for query in [
            "PSMDAX ?",
            "PSDACFIL ?",
            "ILB ?",
            "SSHOS ?",
            "SSALS ?",
            "SSQSNZMA ?",
        ] {
            assert!(marantz.contains(&query) && !denon.contains(&query));
        }
        for query in ["OPALS ?", "SSQS ?"] {
            assert!(denon.contains(&query) && !marantz.contains(&query));
        }
        assert_eq!(denon.len(), 144);
        assert_eq!(marantz.len(), 141);
        for query in ["PSSB: ?", "PSAFD ?", "PSDIL ?"] {
            assert!(denon.contains(&query) && !marantz.contains(&query));
        }
    }

    #[test]
    fn both_brands_ask_for_the_dirac_slots() {
        for brand in BRANDS {
            assert!(state_queries(brand).any(|query| query == "OPDLS ?"));
        }
    }

    #[test]
    fn a_denon_receiver_asks_for_the_quick_selects_and_levels() {
        for wanted in ["SSQS ?", "SSLEV ?"] {
            assert!(state_queries(Brand::Denon).any(|query| query == wanted));
            assert!(!state_queries(Brand::Marantz).any(|query| query == wanted));
        }
    }

    #[test]
    fn both_brands_ask_for_the_sources_sound_modes_and_name() {
        for brand in BRANDS {
            for wanted in ["SSFUN ?", "SSSOD ?", "OPSMLALL ?", "NSFRN ?"] {
                assert!(state_queries(brand).any(|query| query == wanted));
            }
        }
    }

    #[test]
    fn shared_queries_keep_the_same_order() {
        let shared = |brand| -> Vec<&'static str> {
            state_queries(brand)
                .filter(|query| {
                    QUERIES
                        .iter()
                        .any(|q| q.text == *query && matches!(q.brands, Brands::Both))
                })
                .collect()
        };
        assert_eq!(shared(Brand::Denon), shared(Brand::Marantz));
    }

    #[test]
    fn both_brands_ask_for_extended_colorimetry() {
        for brand in BRANDS {
            assert!(state_queries(brand).any(|query| query == "SSINFSIGECI ?"));
        }
    }

    #[test]
    fn the_groups_hold_every_query_once() {
        for brand in BRANDS {
            let mut from_groups: Vec<_> = QueryGroup::ALL
                .iter()
                .flat_map(|group| group_queries(brand, *group))
                .collect();
            let mut all: Vec<_> = state_queries(brand).collect();
            from_groups.sort_unstable();
            all.sort_unstable();
            assert_eq!(from_groups, all);
        }
        assert_eq!(QueryGroup::DEFAULT_ORDER.len(), QueryGroup::ALL.len());
    }

    #[test]
    fn the_default_order_starts_with_the_core_and_the_toggles_come_before_the_signal() {
        for brand in BRANDS {
            let all: Vec<_> = state_queries(brand).collect();
            let position = |wanted| all.iter().position(|query| *query == wanted).unwrap();
            assert_eq!(all[0], "SI?");
            assert!(position("MS?") < position("SSFUN ?"));
            assert!(position("SSFUN ?") < position("PSTONE CTRL ?"));
            assert!(position("PSTONE CTRL ?") < position("SSINFSIGRES ?"));
            assert!(position("SSINFSIGRES ?") < position("PSBAS ?"));
            assert!(position("PSBAS ?") < position("SSVCTZMA ?"));
        }
    }

    #[test]
    fn a_client_chooses_and_orders_the_groups() {
        static CHOSEN: [QueryGroup; 3] = [Signal, Core, Signal];
        let chosen: Vec<_> = queries(Brand::Denon, &CHOSEN).collect();
        let signal = group_queries(Brand::Denon, Signal).count();
        let core = group_queries(Brand::Denon, Core).count();
        // A group that is listed twice counts once.
        assert_eq!(chosen.len(), signal + core);
        assert_eq!(chosen[0], "SSINFSIGRES ?");
        assert_eq!(chosen[signal], "SI?");
        assert!(queries(Brand::Denon, &[]).next().is_none());
    }

    #[test]
    fn brand_queries_stay_in_their_group() {
        assert!(group_queries(Brand::Denon, Core).any(|q| q == "MSQUICK ?"));
        assert!(group_queries(Brand::Marantz, Core).any(|q| q == "MSSMART ?"));
        assert!(!group_queries(Brand::Denon, Core).any(|q| q == "MSSMART ?"));
    }

    #[test]
    fn the_parts_of_the_signal_group_cover_it() {
        for brand in [Brand::Denon, Brand::Marantz] {
            let all = group_queries(brand, Signal).count();
            let parts: usize = [
                SignalPart::Video,
                SignalPart::Channels,
                SignalPart::AudioInfo,
                SignalPart::TelnetOnly,
                SignalPart::Colorimetry,
            ]
            .into_iter()
            .map(|part| signal_queries(brand, part).count())
            .sum();
            assert_eq!(all, parts);
            assert_eq!(signal_queries(brand, SignalPart::Channels).count(), 2);
            assert!(signal_queries(brand, SignalPart::AudioInfo).any(|q| q == "SYSDA ?"));
            assert_eq!(signal_queries(brand, SignalPart::AudioInfo).count(), 3);
            assert!(signal_queries(brand, SignalPart::Colorimetry).eq(["SSINFSIGECI ?"]));
            // In the group, it comes after the video queries.
            assert_eq!(group_queries(brand, Signal).last(), Some("SSINFSIGECI ?"));
            assert!(!signal_queries(brand, SignalPart::Video).any(|q| q == "SYSDA ?"));
        }
    }

    #[test]
    fn queries_after_appcommand_are_not_what_it_answers() {
        let denon: Vec<_> = after_appcommand_queries(Brand::Denon).collect();
        let marantz: Vec<_> = after_appcommand_queries(Brand::Marantz).collect();
        assert_eq!(denon[0], "MSQUICK ?");
        assert_eq!(marantz[0], "MSSMART ?");
        assert!(denon.ends_with(&["OPDLS ?", "PSDIRAC ?", "OPSMLALL ?"]));
        for list in [&denon, &marantz] {
            // AppCommand answers these.
            for answered in ["MV?", "MU?", "SI?", "MS?", "Z2?"] {
                assert!(!list.contains(&answered), "{answered}");
            }
            assert!(list.contains(&"PSDYNEQ ?"));
        }
    }

    #[test]
    fn the_start_groups_are_the_first_of_the_default_order() {
        assert!(QueryGroup::DEFAULT_ORDER.starts_with(QueryGroup::START_ORDER));
    }

    #[test]
    fn the_start_groups_have_what_the_controls_check() {
        // The controls of the client read these values before they send a
        // command. If the state did not know them, a command that the receiver
        // ignores would be sent: the selected Dirac filter blocks tone control
        // and the Audyssey settings, and Dynamic EQ must be on for some
        // Audyssey settings.
        for brand in [Brand::Denon, Brand::Marantz] {
            let start: Vec<_> = queries(brand, QueryGroup::START_ORDER).collect();
            let first: Vec<_> = after_appcommand_queries(brand).collect();
            for needed in ["PSDIRAC ?", "OPDLS ?", "PSDYNEQ ?", "PSTONE CTRL ?"] {
                assert!(
                    start.contains(&needed),
                    "{needed} is not in the start groups"
                );
                assert!(
                    first.contains(&needed),
                    "{needed} is not in the first queries"
                );
            }
        }
    }

    #[test]
    fn queries_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for query in &QUERIES {
            assert!(seen.insert(query.text), "{}", query.text);
        }
    }
}
