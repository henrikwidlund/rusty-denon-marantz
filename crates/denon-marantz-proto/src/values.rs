//! Receiver setting values with their telnet and AppCommand codes.

code_enum! {
    /// Audio restorer level.
    pub enum AudioRestorer {
        Off = "OFF", "Off";
        Low = "LOW", "Low";
        Medium = "MED", "Medium";
        High = "HI", "High";
    }
}

code_enum! {
    /// Audyssey DSX: the speakers that it adds.
    pub enum AudysseyDsx {
        Off = "OFF", "Off";
        Height = "ONH", "Height";
        Wide = "ONW", "Wide";
        HeightAndWide = "ONHW", "Height + Wide";
    }
}

code_enum! {
    /// Auro-3D mode.
    pub enum Auro3dMode {
        Direct = "DRCT", "Direct";
        ChannelExpansion = "EXP", "Channel Expansion";
    }
}

code_enum! {
    /// Auro-Matic 3D preset.
    pub enum AuroMatic3dPreset {
        Small = "SMA", "Small";
        Medium = "MED", "Medium";
        Large = "LAR", "Large";
        Speech = "SPE", "Speech";
        Movie = "MOV", "Movie";
    }
}

code_enum! {
    /// Bluetooth transmitter output mode.
    pub enum BluetoothOutputMode {
        BluetoothAndSpeakers = "SP", "Bluetooth + Speakers";
        BluetoothOnly = "BT", "Bluetooth Only";
    }
}

code_enum! {
    /// DAC filter mode (Marantz).
    pub enum DacFilter {
        Mode1 = "MODE1", "Mode 1";
        Mode2 = "MODE2", "Mode 2";
    }
}

code_enum! {
    /// Dialog enhancer level.
    pub enum DialogEnhancerLevel {
        Off = "OFF", "Off";
        Low = "LOW", "Low";
        Medium = "MED", "Medium";
        High = "HIGH", "High";
    }
}

code_enum! {
    /// Digital input codec.
    pub enum DigitalCodec {
        Auto = "AUTO", "Auto";
        Pcm = "PCM", "PCM";
        Dts = "DTS", "DTS";
    }
}

code_enum! {
    /// Color space of an HDMI video signal. Some receivers send `9` for
    /// 4:2:2 with BT.2020: that is [`ColorSpace::Yuv422`] and
    /// [`SignalState::bt2020`](crate::SignalState::bt2020).
    pub enum ColorSpace {
        Yuv422 = "1", "4:2:2";
        Yuv444 = "2", "4:4:4";
        Rgb = "3", "RGB";
        Yuv420 = "8", "4:2:0";
    }
}

code_enum! {
    /// Audio input mode of the main zone. The receiver also has a state with
    /// no input, but it only reports that and cannot be set.
    pub enum InputMode {
        Auto = "AUTO", "Auto";
        Hdmi = "HDMI", "HDMI";
        Digital = "DIGITAL", "Digital";
        Analog = "ANALOG", "Analog";
    }
}

code_enum! {
    /// Aspect ratio of the picture. Older receivers only.
    pub enum AspectRatio {
        Normal = "NRM", "Normal (4:3)";
        Full = "FUL", "Full (16:9)";
    }
}

code_enum! {
    /// Output resolution of the video conversion.
    pub enum VideoResolution {
        Auto = "AUTO", "Auto";
        P480 = "48P", "480p/576p";
        I1080 = "10I", "1080i";
        P720 = "72P", "720p";
        P1080 = "10P", "1080p";
        P1080At24 = "10P24", "1080p 24 Hz";
        K4 = "4K", "4K 24/60/50";
        K4Fast = "4KF", "4K 60/50";
        K8 = "8K", "8K";
    }
}

code_enum! {
    /// Front speaker output. The receiver needs "Amp assign = Front B" in its
    /// setup.
    pub enum FrontSpeakers {
        A = "SPA", "Speaker A";
        B = "SPB", "Speaker B";
        Both = "A+B", "Speakers A and B";
    }
}

code_enum! {
    /// Speaker preset of the receiver.
    pub enum SpeakerPreset {
        One = "1", "Preset 1";
        Two = "2", "Preset 2";
    }
}

code_enum! {
    /// Band of the tuner.
    pub enum TunerBand {
        Am = "AM", "AM";
        Fm = "FM", "FM";
        Dab = "DAB", "DAB";
    }
}

code_enum! {
    /// Tuning mode of the tuner.
    pub enum TunerMode {
        Auto = "AUTO", "Automatic";
        Manual = "MANUAL", "Manual";
    }
}

code_enum! {
    /// Noise reduction of the picture.
    pub enum NoiseReduction {
        Off = "OFF", "Off";
        Low = "LOW", "Low";
        Mid = "MID", "Middle";
        High = "HI", "High";
    }
}

impl PictureMode {
    /// Code for the telnet command and event. The code of `Custom` is `CTM`
    /// in the protocol spec.
    #[must_use]
    pub const fn telnet_code(self) -> &'static str {
        self.code()
    }

    /// Parses the telnet code of an event. The code `CSTM` of other sources is
    /// the custom mode too.
    #[must_use]
    pub fn from_telnet(code: &str) -> Option<Self> {
        if code == "CSTM" {
            Some(Self::Custom)
        } else {
            Self::from_code(code)
        }
    }
}

code_enum! {
    /// Dirac Live filter slot.
    pub enum DiracFilter {
        Off = "OFF", "Off";
        Slot1 = "1", "Slot 1";
        Slot2 = "2", "Slot 2";
        Slot3 = "3", "Slot 3";
    }
}

impl DiracFilter {
    /// The filter of slot 1, 2 or 3. `None` for another number.
    #[must_use]
    pub const fn from_slot_number(number: u8) -> Option<Self> {
        match number {
            1 => Some(Self::Slot1),
            2 => Some(Self::Slot2),
            3 => Some(Self::Slot3),
            _ => None,
        }
    }
}

code_enum! {
    /// Bits for each color of an HDMI video signal.
    pub enum PixelDepth {
        Unknown = "0", "Unknown";
        Bits8 = "1", "8-bit";
        Bits10 = "2", "10-bit";
        Bits12 = "3", "12-bit";
    }
}

code_enum! {
    /// Speakers used for effect channels.
    pub enum EffectSpeakerSelection {
        Floor = "FL", "Floor";
        Front = "FR", "Front";
        FrontHeight = "FH", "Front Height";
        FrontWide = "FW", "Front Wide";
        FrontHeightAndWide = "HW", "Front Height + Front Wide";
        HeightAndFloor = "HF", "Height + Floor";
        SurroundBack = "SB", "Surround Back";
        SurroundBackAndFrontHeight = "BH", "Surround Back + Front Height";
        SurroundBackAndFrontWide = "BW", "Surround Back + Front Wide";
    }
}

code_enum! {
    /// Highest resolution that the TV and the cables allow, from the HDMI
    /// diagnostics of the receiver.
    pub enum MaxHdmiResolution {
        NoLimit = "NO", "No Limit";
        K4Gbps18 = "4K18G", "4K18Gbps";
        K4Gbps9 = "4K9G", "4K9Gbps";
        P1080 = "1080P", "1080p";
        P720 = "720P", "720p";
        P480 = "480P", "480p";
    }
}

dual_code_enum! {
    /// HDMI monitor output. The `AppCommand` codes are those of
    /// `GetOutputSettings`.
    pub enum HdmiOutput {
        Auto = "MONIAUTO", Some("3"), "Auto";
        Hdmi1 = "MONI1", Some("1"), "HDMI1";
        Hdmi2 = "MONI2", Some("2"), "HDMI2";
    }
}

code_enum! {
    /// Front panel illumination (Marantz).
    pub enum Illumination {
        Auto = "AUTO", "Auto";
        Bright = "BRI", "Bright";
        Dim = "DIM", "Dim";
        Dark = "DAR", "Dark";
        Off = "OFF", "Off";
    }
}

code_enum! {
    /// MDAX level (Marantz).
    pub enum Mdax {
        Off = "OFF", "Off";
        Low = "LOW", "Low";
        Medium = "MED", "Medium";
        High = "HI", "High";
    }
}

code_enum! {
    /// Picture mode.
    pub enum PictureMode {
        Off = "OFF", "Off";
        Brilliant = "BRL", "Brilliant";
        Custom = "CTM", "Custom";
        Game = "GAM", "Game";
        IsfDay = "DAY", "ISF Day";
        IsfNight = "NGT", "ISF Night";
        Movie = "MOV", "Movie";
        Standard = "STD", "Standard";
        Stream = "STM", "Stream";
        Vivid = "VVD", "Vivid";
    }
}

code_enum! {
    /// Video processing mode.
    pub enum VideoProcessingMode {
        Auto = "AUTO", "Auto";
        Game = "GAME", "Game";
        Movie = "MOVI", "Movie";
        Bypass = "BYP", "Bypass";
    }
}

code_enum! {
    /// Dynamic range compression.
    pub enum Drc {
        Auto = "AUTO", "AUTO";
        Low = "LOW", "LOW";
        Mid = "MID", "MID";
        High = "HI", "HI";
        Off = "OFF", "OFF";
    }
}

code_enum! {
    /// HDMI audio decode target.
    pub enum HdmiAudioDecode {
        Amp = "AMP", "AMP";
        Tv = "TV", "TV";
    }
}

code_enum! {
    /// Height gain of Dolby `PLIIz`.
    pub enum PliizHeightGain {
        Low = "LOW", "Low";
        Medium = "MID", "Medium";
        High = "HI", "High";
    }
}

code_enum! {
    /// IMAX mode.
    pub enum ImaxMode {
        Auto = "AUTO", "Auto";
        On = "ON", "On";
        Off = "OFF", "Off";
    }
}

code_enum! {
    /// IMAX audio settings.
    pub enum ImaxAudio {
        Auto = "AUTO", "Auto";
        Manual = "MANUAL", "Manual";
    }
}

code_enum! {
    /// Subwoofer output in IMAX mode.
    pub enum ImaxSubwooferOutput {
        LfeAndMain = "L+M", "LFE + Main";
        Lfe = "LFE", "LFE";
    }
}

code_enum! {
    /// Surround level compensation.
    pub enum SurroundLevelCompensation {
        Off = "OFF", "Off";
        Light = "LIT", "Light";
        Medium = "MED", "Medium";
        Heavy = "HEV", "Heavy";
    }
}

code_enum! {
    /// How far the volume goes down when a zone is muted.
    pub enum MuteLevel {
        Full = "MUT", "Full";
        Minus40Db = "040", "-40 dB";
        Minus20Db = "060", "-20 dB";
    }
}

code_enum! {
    /// How the receiver shows the volume.
    pub enum VolumeDisplay {
        Relative = "REL", "Relative";
        Absolute = "ABS", "Absolute";
    }
}

code_enum! {
    /// Channel mode of zone 2 or 3.
    pub enum ZoneChannelMode {
        Stereo = "ST", "Stereo";
        Mono = "MONO", "Mono";
    }
}

code_enum! {
    /// HDMI audio of zone 2 or 3.
    pub enum ZoneHdmiAudio {
        Through = "THR", "Through";
        Pcm = "PCM", "PCM";
    }
}

code_enum! {
    /// Room size.
    pub enum RoomSize {
        Small = "S", "S";
        MediumSmall = "MS", "MS";
        Medium = "M", "M";
        MediumLarge = "ML", "ML";
        Large = "L", "L";
    }
}

dual_code_enum! {
    /// Auto standby time.
    pub enum AutoStandby {
        Off = "OFF", Some("0"), "OFF";
        Minutes15 = "15M", Some("15"), "15M";
        Minutes30 = "30M", Some("30"), "30M";
        Minutes60 = "60M", Some("60"), "60M";
        Hours2 = "2H", Some("120"), "2H";
        Hours4 = "4H", Some("240"), "4H";
        Hours8 = "8H", Some("480"), "8H";
    }
}

dual_code_enum! {
    /// Display dimmer mode.
    pub enum DimmerMode {
        Off = "OFF", Some("0"), "Off";
        Dark = "DAR", Some("1"), "Dark";
        Dim = "DIM", Some("2"), "Dim";
        Bright = "BRI", Some("3"), "Bright";
    }
}

dual_code_enum! {
    /// Dynamic volume setting.
    pub enum DynamicVolume {
        Off = "OFF", Some("0"), "Off";
        Light = "LIT", Some("1"), "Light";
        Medium = "MED", Some("2"), "Medium";
        Heavy = "HEV", Some("3"), "Heavy";
    }
}

dual_code_enum! {
    /// Eco mode.
    pub enum EcoMode {
        Off = "OFF", Some("0"), "Off";
        On = "ON", Some("1"), "On";
        Auto = "AUTO", Some("2"), "Auto";
    }
}

dual_code_enum! {
    /// Audyssey MultEQ mode.
    pub enum MultiEq {
        Off = "OFF", Some("0"), "Off";
        Flat = "FLAT", Some("1"), "Flat";
        LrBypass = "BYP.LR", Some("2"), "L/R Bypass";
        Reference = "AUDYSSEY", Some("3"), "Reference";
        Manual = "MANUAL", None, "Manual";
    }
}

dual_code_enum! {
    /// Audyssey dynamic EQ reference level offset.
    pub enum ReferenceLevelOffset {
        Db0 = "0", Some("0"), "0dB";
        Db5 = "5", Some("1"), "+5dB";
        Db10 = "10", Some("2"), "+10dB";
        Db15 = "15", Some("3"), "+15dB";
    }
}

code_enum! {
    /// Power state of a zone.
    pub enum Power {
        On = "ON", "On";
        Standby = "STANDBY", "Standby";
        Off = "OFF", "Off";
    }
}

/// A trigger output of the receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Trigger {
    /// Trigger output 1.
    One,
    /// Trigger output 2.
    Two,
    /// Trigger output 3.
    Three,
}

impl Trigger {
    /// All trigger outputs.
    pub const ALL: [Self; 3] = [Self::One, Self::Two, Self::Three];

    /// Number of the output, from 1 to 3.
    #[must_use]
    pub const fn number(self) -> u8 {
        self as u8 + 1
    }

    /// The output with this number. `None` for a number that is not from 1 to 3.
    #[must_use]
    pub const fn from_number(number: u8) -> Option<Self> {
        match number {
            1 => Some(Self::One),
            2 => Some(Self::Two),
            3 => Some(Self::Three),
            _ => None,
        }
    }
}

/// What the panel lock locks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PanelLock {
    /// Nothing is locked.
    Off,
    /// The buttons on the panel are locked, but not the master volume.
    Panel,
    /// The buttons on the panel and the master volume are locked.
    PanelAndVolume,
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! round_trip {
        ($($ty:ident),+ $(,)?) => {$(
            for &value in $ty::ALL {
                assert_eq!($ty::from_code(value.code()), Some(value), stringify!($ty));
                assert!(!value.label().is_empty());
            }
            assert_eq!($ty::from_code("no such code"), None);
        )+};
    }

    macro_rules! dual_round_trip {
        ($($ty:ident),+ $(,)?) => {$(
            for &value in $ty::ALL {
                assert_eq!($ty::from_telnet(value.telnet_code()), Some(value), stringify!($ty));
                if let Some(code) = value.appcommand_code() {
                    assert_eq!($ty::from_appcommand(code), Some(value), stringify!($ty));
                }
            }
            assert_eq!($ty::from_telnet("no such code"), None);
            assert_eq!($ty::from_appcommand("no such code"), None);
            assert_eq!($ty::from_any("no such code"), None);
            for &value in $ty::ALL {
                // A code that is valid in both kinds must mean the same value.
                let codes = [Some(value.telnet_code()), value.appcommand_code()];
                for code in codes.into_iter().flatten() {
                    assert_eq!($ty::from_any(code), Some(value), "{} {code}", stringify!($ty));
                }
            }
        )+};
    }

    #[test]
    fn single_code_enums_round_trip() {
        round_trip!(
            AspectRatio,
            AudioRestorer,
            AudysseyDsx,
            Auro3dMode,
            AuroMatic3dPreset,
            BluetoothOutputMode,
            ColorSpace,
            DacFilter,
            DialogEnhancerLevel,
            DigitalCodec,
            DiracFilter,
            Drc,
            EffectSpeakerSelection,
            HdmiAudioDecode,
            PliizHeightGain,
            MaxHdmiResolution,
            FrontSpeakers,
            Illumination,
            InputMode,
            Mdax,
            NoiseReduction,
            PictureMode,
            PixelDepth,
            Power,
            RoomSize,
            SpeakerPreset,
            TunerBand,
            TunerMode,
            VideoProcessingMode,
            VideoResolution,
        );
    }

    #[test]
    fn dual_code_enums_round_trip() {
        dual_round_trip!(
            AutoStandby,
            DimmerMode,
            DynamicVolume,
            EcoMode,
            HdmiOutput,
            MultiEq,
            ReferenceLevelOffset,
        );
    }

    #[test]
    fn single_code_known_answers() {
        assert_eq!(AudioRestorer::from_code("MED"), Some(AudioRestorer::Medium));
        assert_eq!(PictureMode::IsfNight.code(), "NGT");
        assert_eq!(DiracFilter::Slot3.code(), "3");
        assert_eq!(ColorSpace::from_code("9"), None);
        assert_eq!(ColorSpace::from_code("0"), None);
        assert_eq!(
            PixelDepth::from_code("2").map(PixelDepth::label),
            Some("10-bit")
        );
        assert_eq!(VideoProcessingMode::Movie.code(), "MOVI");
        assert_eq!(Drc::High.code(), "HI");
    }

    #[test]
    fn dual_code_known_answers() {
        assert_eq!(DimmerMode::from_telnet("DAR"), Some(DimmerMode::Dark));
        assert_eq!(DimmerMode::from_appcommand("3"), Some(DimmerMode::Bright));
        assert_eq!(DimmerMode::from_appcommand("DAR"), None);
        assert_eq!(HdmiOutput::from_telnet("MONIAUTO"), Some(HdmiOutput::Auto));
        assert_eq!(HdmiOutput::from_appcommand("3"), Some(HdmiOutput::Auto));
        assert_eq!(HdmiOutput::Hdmi2.label(), "HDMI2");
        assert_eq!(
            AutoStandby::from_appcommand("120"),
            Some(AutoStandby::Hours2)
        );
        assert_eq!(AutoStandby::Minutes15.telnet_code(), "15M");
        assert_eq!(EcoMode::from_appcommand("2"), Some(EcoMode::Auto));
        assert_eq!(MultiEq::from_telnet("AUDYSSEY"), Some(MultiEq::Reference));
        assert_eq!(MultiEq::Manual.appcommand_code(), None);
        assert_eq!(MultiEq::LrBypass.label(), "L/R Bypass");
        assert_eq!(
            ReferenceLevelOffset::from_telnet("15"),
            Some(ReferenceLevelOffset::Db15)
        );
        assert_eq!(
            ReferenceLevelOffset::from_appcommand("1"),
            Some(ReferenceLevelOffset::Db5)
        );
        assert_eq!(
            DynamicVolume::from_telnet("HEV"),
            Some(DynamicVolume::Heavy)
        );
    }
}
