//! Speaker channels and channel levels.

code_enum! {
    /// Speaker channel.
    pub enum Channel {
        FrontLeft = "FL", "Front Left";
        FrontRight = "FR", "Front Right";
        Center = "C", "Center";
        Subwoofer = "SW", "Subwoofer";
        Subwoofer2 = "SW2", "Subwoofer 2";
        Subwoofer3 = "SW3", "Subwoofer 3";
        Subwoofer4 = "SW4", "Subwoofer 4";
        SurroundLeft = "SL", "Surround Left";
        SurroundRight = "SR", "Surround Right";
        SurroundBack = "SB", "Surround Back";
        SurroundBackLeft = "SBL", "Surround Back Left";
        SurroundBackRight = "SBR", "Surround Back Right";
        FrontHeightLeft = "FHL", "Front Height Left";
        FrontHeightRight = "FHR", "Front Height Right";
        FrontWideLeft = "FWL", "Front Wide Left";
        FrontWideRight = "FWR", "Front Wide Right";
        TopFrontLeft = "TFL", "Top Front Left";
        TopFrontRight = "TFR", "Top Front Right";
        TopMiddleLeft = "TML", "Top Middle Left";
        TopMiddleRight = "TMR", "Top Middle Right";
        TopRearLeft = "TRL", "Top Rear Left";
        TopRearRight = "TRR", "Top Rear Right";
        RearHeightLeft = "RHL", "Rear Height Left";
        RearHeightRight = "RHR", "Rear Height Right";
        FrontDolbyLeft = "FDL", "Front Dolby Left";
        FrontDolbyRight = "FDR", "Front Dolby Right";
        SurroundDolbyLeft = "SDL", "Surround Dolby Left";
        SurroundDolbyRight = "SDR", "Surround Dolby Right";
        BackDolbyLeft = "BDL", "Back Dolby Left";
        BackDolbyRight = "BDR", "Back Dolby Right";
        SurroundHeightLeft = "SHL", "Surround Height Left";
        SurroundHeightRight = "SHR", "Surround Height Right";
        TopSurround = "TS", "Top Surround";
        CenterHeight = "CH", "Center Height";
    }
}

code_enum! {
    /// Subwoofer output.
    pub enum Subwoofer {
        Subwoofer1 = "SWL", "Subwoofer";
        Subwoofer2 = "SWL2", "Subwoofer 2";
        Subwoofer3 = "SWL3", "Subwoofer 3";
        Subwoofer4 = "SWL4", "Subwoofer 4";
    }
}

/// Level codes in steps of 0.5 dB, from -12.0 dB to +12.0 dB.
static LEVEL_CODES: [&str; 49] = [
    "38", "385", "39", "395", "40", "405", "41", "415", "42", "425", "43", "435", "44", "445",
    "45", "455", "46", "465", "47", "475", "48", "485", "49", "495", "50", "505", "51", "515",
    "52", "525", "53", "535", "54", "545", "55", "555", "56", "565", "57", "575", "58", "585",
    "59", "595", "60", "605", "61", "615", "62",
];

/// Channel or subwoofer level from -12.0 dB to +12.0 dB in steps of 0.5 dB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChannelLevel(i8);

impl ChannelLevel {
    /// Lowest level, -12.0 dB.
    pub const MIN: Self = Self(-24);
    /// Highest level, +12.0 dB.
    pub const MAX: Self = Self(24);

    /// Creates a level from steps of 0.5 dB, for example `-3` is -1.5 dB.
    #[must_use]
    pub const fn from_half_db(steps: i8) -> Option<Self> {
        if steps >= Self::MIN.0 && steps <= Self::MAX.0 {
            Some(Self(steps))
        } else {
            None
        }
    }

    /// Creates a level from dB. Returns `None` if the value is not a multiple of 0.5 or is out of range.
    #[must_use]
    #[allow(clippy::cast_possible_truncation)] // Range is checked before the cast.
    pub fn from_db(db: f32) -> Option<Self> {
        let doubled = db * 2.0;
        let steps = doubled.round();
        if (doubled - steps).abs() > f32::EPSILON || !(-24.0..=24.0).contains(&steps) {
            return None;
        }
        Some(Self(steps as i8))
    }

    /// Level in dB.
    #[must_use]
    pub fn db(self) -> f32 {
        f32::from(self.0) / 2.0
    }

    /// Level in steps of 0.5 dB.
    #[must_use]
    pub const fn half_db(self) -> i8 {
        self.0
    }

    /// Code used by the receiver.
    #[must_use]
    pub fn code(self) -> &'static str {
        // The range is checked in the constructors.
        LEVEL_CODES[usize::try_from(i16::from(self.0) + 24).unwrap_or(0)]
    }

    /// Parses a code used by the receiver.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        let index = LEVEL_CODES.iter().position(|c| *c == code)?;
        i8::try_from(index).ok().map(|i| Self(i - 24))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_enums_round_trip() {
        for &channel in Channel::ALL {
            assert_eq!(Channel::from_code(channel.code()), Some(channel));
        }
        for &sub in Subwoofer::ALL {
            assert_eq!(Subwoofer::from_code(sub.code()), Some(sub));
        }
        assert_eq!(Channel::ALL.len(), 34);
    }

    #[test]
    fn channel_known_answers() {
        assert_eq!(Channel::from_code("SBL"), Some(Channel::SurroundBackLeft));
        assert_eq!(Channel::CenterHeight.code(), "CH");
        assert_eq!(Subwoofer::Subwoofer3.code(), "SWL3");
    }

    #[test]
    fn level_codes_follow_the_scheme() {
        for steps in -24..=24_i8 {
            let level = ChannelLevel::from_half_db(steps).unwrap();
            let total = 100 + i32::from(steps);
            let expected = format!("{}{}", total / 2, if total % 2 == 1 { "5" } else { "" });
            assert_eq!(level.code(), expected);
            assert_eq!(ChannelLevel::from_code(level.code()), Some(level));
        }
    }

    #[test]
    fn level_known_answers() {
        assert_eq!(
            ChannelLevel::from_code("38").map(ChannelLevel::db),
            Some(-12.0)
        );
        assert_eq!(
            ChannelLevel::from_code("495").map(ChannelLevel::db),
            Some(-0.5)
        );
        assert_eq!(
            ChannelLevel::from_code("50").map(ChannelLevel::db),
            Some(0.0)
        );
        assert_eq!(
            ChannelLevel::from_code("62").map(ChannelLevel::db),
            Some(12.0)
        );
        assert_eq!(ChannelLevel::from_db(-11.5).unwrap().code(), "385");
        assert_eq!(ChannelLevel::from_code("37"), None);
        assert_eq!(ChannelLevel::from_code("625"), None);
        assert_eq!(ChannelLevel::from_code(""), None);
    }

    #[test]
    fn level_from_db_checks_input() {
        assert_eq!(ChannelLevel::from_db(12.0), Some(ChannelLevel::MAX));
        assert_eq!(ChannelLevel::from_db(-12.0), Some(ChannelLevel::MIN));
        assert_eq!(ChannelLevel::from_db(12.5), None);
        assert_eq!(ChannelLevel::from_db(0.3), None);
        assert_eq!(ChannelLevel::from_db(f32::NAN), None);
        assert_eq!(ChannelLevel::from_half_db(25), None);
        assert_eq!(ChannelLevel::from_half_db(-25), None);
    }
}
