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
    /// Dirac Live filter slot.
    pub enum DiracFilter {
        Off = "OFF", "Off";
        Slot1 = "1", "Slot 1";
        Slot2 = "2", "Slot 2";
        Slot3 = "3", "Slot 3";
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
    /// HDMI monitor output.
    pub enum HdmiOutput {
        Auto = "MONIAUTO", "Auto";
        Hdmi1 = "MONI1", "HDMI1";
        Hdmi2 = "MONI2", "HDMI2";
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
        Custom = "CSTM", "Custom";
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
        )+};
    }

    #[test]
    fn single_code_enums_round_trip() {
        round_trip!(
            AudioRestorer,
            Auro3dMode,
            AuroMatic3dPreset,
            BluetoothOutputMode,
            DacFilter,
            DialogEnhancerLevel,
            DigitalCodec,
            DiracFilter,
            Drc,
            EffectSpeakerSelection,
            HdmiAudioDecode,
            HdmiOutput,
            Illumination,
            Mdax,
            PictureMode,
            RoomSize,
            VideoProcessingMode,
        );
    }

    #[test]
    fn dual_code_enums_round_trip() {
        dual_round_trip!(
            AutoStandby,
            DimmerMode,
            DynamicVolume,
            EcoMode,
            MultiEq,
            ReferenceLevelOffset,
        );
    }

    #[test]
    fn single_code_known_answers() {
        assert_eq!(AudioRestorer::from_code("MED"), Some(AudioRestorer::Medium));
        assert_eq!(HdmiOutput::from_code("MONIAUTO"), Some(HdmiOutput::Auto));
        assert_eq!(HdmiOutput::Hdmi2.label(), "HDMI2");
        assert_eq!(PictureMode::IsfNight.code(), "NGT");
        assert_eq!(DiracFilter::Slot3.code(), "3");
        assert_eq!(VideoProcessingMode::Movie.code(), "MOVI");
        assert_eq!(Drc::High.code(), "HI");
    }

    #[test]
    fn dual_code_known_answers() {
        assert_eq!(DimmerMode::from_telnet("DAR"), Some(DimmerMode::Dark));
        assert_eq!(DimmerMode::from_appcommand("3"), Some(DimmerMode::Bright));
        assert_eq!(DimmerMode::from_appcommand("DAR"), None);
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
