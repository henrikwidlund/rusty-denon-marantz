//! Queries that ask a receiver for its state.

use QueryGroup::{Core, Device, Names, Picture, Signal, Sound, Speakers, Toggles, Zones};

use crate::brand::Brand;

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
        /// Auto lip sync.
        AutoLipSync,
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
        /// Center gain (Marantz).
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
}

impl Query {
    /// The query text for this brand. `None` if the brand has no query for
    /// the value.
    #[must_use]
    pub fn text(self, brand: Brand) -> Option<&'static str> {
        QUERIES
            .iter()
            .find(|entry| entry.query == self && entry.is_for(brand))
            .map(|entry| entry.text)
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
    ///    video select, video and picture settings.
    /// 6. [`Sound`](Self::Sound): the sound settings and levels.
    /// 7. [`Zones`](Self::Zones): the volume settings and levels of the zones.
    /// 8. [`Picture`](Self::Picture): the picture mode and the picture
    ///    adjustments. Only older receivers answer these.
    /// 9. [`Speakers`](Self::Speakers): the speaker configuration, the
    ///    crossovers and the graphic EQ. Only Denon receivers answer these.
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

/// Which read answers a query of the [`Signal`](QueryGroup::Signal) group.
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
    /// The extended colorimetry (BT.2020). Only telnet answers it. Ask it
    /// after the video queries, so that its answer comes after theirs.
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

static QUERIES: [Entry; 130] = [
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
    marantz(Sound, Query::CenterGain, "PSCEG ?"), // Center gain
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
    both(Picture, Query::Contrast, "PVCN ?"), // Contrast
    both(Picture, Query::Brightness, "PVBR ?"), // Brightness
    both(Picture, Query::Saturation, "PVST ?"), // Saturation
    both(Picture, Query::NoiseReduction, "PVDNR ?"), // Noise Reduction
    both(Picture, Query::Enhancer, "PVENH ?"), // Enhancer
];

#[cfg(test)]
mod tests {
    use super::*;

    const BRANDS: [Brand; 2] = [Brand::Denon, Brand::Marantz];

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
    fn speaker_queries_are_last_and_for_denon() {
        let denon: Vec<_> = state_queries(Brand::Denon).collect();
        let marantz: Vec<_> = state_queries(Brand::Marantz).collect();
        let tail: Vec<_> = denon[denon.len() - 3..].to_vec();
        assert_eq!(tail, ["SSSPC ?", "SSCFR ?", "SSAEQLRS ?"]);
        assert!(!marantz.contains(&"SSAEQLRS ?"));
        assert!(denon.contains(&"SD?") && marantz.contains(&"SD?"));
        assert_eq!(QueryGroup::DEFAULT_ORDER.last(), Some(&Speakers));
    }

    #[test]
    fn brand_specific_queries() {
        let denon: Vec<_> = state_queries(Brand::Denon).collect();
        let marantz: Vec<_> = state_queries(Brand::Marantz).collect();
        assert!(denon.contains(&"MSQUICK ?"));
        assert!(!denon.contains(&"MSSMART ?"));
        assert!(marantz.contains(&"MSSMART ?"));
        for query in ["PSMDAX ?", "PSDACFIL ?", "ILB ?", "SSHOS ?", "SSQSNZMA ?"] {
            assert!(marantz.contains(&query) && !denon.contains(&query));
        }
        for query in ["OPALS ?", "SSQS ?"] {
            assert!(denon.contains(&query) && !marantz.contains(&query));
        }
        assert_eq!(denon.len(), 121);
        assert_eq!(marantz.len(), 121);
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
