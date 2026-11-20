//! Master volume.

use core::fmt;

/// The volume that a zone has when it is switched on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PowerOnVolume {
    /// The zone is muted.
    Muted,
    /// The zone has the volume that it had when it was switched off.
    Last,
    /// The zone has this volume.
    Level(Volume),
}

impl PowerOnVolume {
    /// Parses the value of a telnet event: `MUT`, `LAS` or whole dB above
    /// -80 dB, for example `35` for -45.0 dB.
    #[must_use]
    pub fn from_telnet(code: &str) -> Option<Self> {
        match code.trim() {
            "MUT" => Some(Self::Muted),
            "LAS" => Some(Self::Last),
            code => Volume::from_whole_db_above_min(code).map(Self::Level),
        }
    }
}

/// How the volume of a zone is set: with the volume control, or fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum VolumeMode {
    /// The volume control changes the volume.
    Variable,
    /// The volume of the zone is fixed at this level.
    Fixed(Volume),
}

impl VolumeMode {
    /// Parses the value of a telnet event: `VAR` or the fixed volume in whole
    /// dB above -80 dB, for example `10` for -70.0 dB.
    #[must_use]
    pub fn from_telnet(code: &str) -> Option<Self> {
        if code.trim() == "VAR" {
            Some(Self::Variable)
        } else {
            Volume::from_whole_db_above_min(code).map(Self::Fixed)
        }
    }
}

/// Master volume from -80.0 dB to +18.0 dB in steps of 0.5 dB.
///
/// The telnet code is the volume above -80 dB: two digits for whole dB and
/// three digits for a half step, for example `43` is -37.0 dB and `435` is
/// -36.5 dB. [`Display`](fmt::Display) writes the telnet code.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Volume(u8);

impl Volume {
    /// Lowest volume, -80.0 dB.
    pub const MIN: Self = Self(0);
    /// Highest volume, +18.0 dB.
    pub const MAX: Self = Self(196);

    /// Creates a volume from steps of 0.5 dB above -80 dB.
    #[must_use]
    pub const fn from_half_db_above_min(steps: u8) -> Option<Self> {
        if steps <= Self::MAX.0 {
            Some(Self(steps))
        } else {
            None
        }
    }

    /// Creates a volume from dB. Rounds to the nearest 0.5 dB.
    /// Returns `None` if the value is out of range or not a number.
    #[must_use]
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // Range is checked before the cast.
    pub fn from_db_rounded(db: f32) -> Option<Self> {
        let steps = ((db + 80.0) * 2.0).round();
        if (0.0..=196.0).contains(&steps) {
            Some(Self(steps as u8))
        } else {
            None
        }
    }

    /// Volume in dB.
    #[must_use]
    pub fn db(self) -> f32 {
        f32::from(self.0) / 2.0 - 80.0
    }

    /// Volume in steps of 0.5 dB above -80 dB.
    #[must_use]
    pub const fn half_db_above_min(self) -> u8 {
        self.0
    }

    /// The volume in whole dB: a half step is raised to the next whole dB.
    #[must_use]
    pub const fn rounded_up_to_whole_db(self) -> Self {
        Self(self.0.next_multiple_of(2))
    }

    /// Parses a volume that is written as whole dB above -80 dB, with any
    /// number of digits: `35` and `035` are -45.0 dB. Zone volume settings
    /// use it. Returns `None` for text that is not a number, and for a number
    /// above 98.
    #[must_use]
    pub fn from_whole_db_above_min(code: &str) -> Option<Self> {
        let code = code.trim();
        if code.is_empty() || !code.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let db: u8 = code.parse().ok()?;
        Self::from_half_db_above_min(db.checked_mul(2)?)
    }

    /// Parses the volume of a telnet event.
    ///
    /// `--` is the lowest volume. Receivers from before about 2013 send `99`
    /// for the lowest volume and `995` for -80.5 dB, so both give the lowest
    /// volume too. Other values above the highest volume give the highest
    /// volume. Returns `None` for text that is not a volume.
    #[must_use]
    pub fn from_telnet(code: &str) -> Option<Self> {
        let volume = Self::from_telnet_clamped(code)?;
        if code.trim().starts_with("99") {
            return Some(Self::MIN);
        }
        Some(volume)
    }

    /// Parses a volume like [`Volume::from_telnet`], but `99`, `990` and `995`
    /// give the highest volume, not the lowest. Used for the volume limit.
    pub(crate) fn from_telnet_clamped(code: &str) -> Option<Self> {
        let code = code.trim();
        if code == "--" {
            return Some(Self::MIN);
        }
        let bytes = code.as_bytes();
        if bytes.is_empty() || !bytes.iter().all(u8::is_ascii_digit) {
            return None;
        }
        let digit = |index: usize| u16::from(bytes[index] - b'0');
        let steps = match bytes.len() {
            1 => digit(0) * 2,
            2 => (digit(0) * 10 + digit(1)) * 2,
            3 => match bytes[2] {
                b'0' => (digit(0) * 10 + digit(1)) * 2,
                b'5' => (digit(0) * 10 + digit(1)) * 2 + 1,
                _ => return None,
            },
            _ => return None,
        };
        // The steps are at most 196 here, so the conversion cannot fail.
        Some(Self(u8::try_from(steps.min(196)).ok()?))
    }
}

impl fmt::Debug for Volume {
    /// Writes the volume in dB, for example `-36.5 dB`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.1} dB", self.db())
    }
}

impl fmt::Display for Volume {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / 2;
        if self.0 % 2 == 1 {
            write!(f, "{whole:02}5")
        } else {
            write!(f, "{whole:02}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_is_the_telnet_code() {
        let code = |db: f32| Volume::from_db_rounded(db).unwrap().to_string();
        assert_eq!(code(-80.0), "00");
        assert_eq!(code(-79.5), "005");
        assert_eq!(code(-37.0), "43");
        assert_eq!(code(-36.5), "435");
        assert_eq!(code(0.0), "80");
        assert_eq!(code(18.0), "98");
    }

    #[test]
    fn debug_shows_db() {
        assert_eq!(format!("{:?}", Volume::MIN), "-80.0 dB");
        assert_eq!(format!("{:?}", Volume::MAX), "18.0 dB");
        assert_eq!(
            format!("{:?}", Volume::from_db_rounded(-36.5).unwrap()),
            "-36.5 dB"
        );
        assert_eq!(format!("{:?}", Some(Volume::MIN)), "Some(-80.0 dB)");
    }

    #[test]
    fn telnet_known_answers() {
        let db = |code: &str| Volume::from_telnet(code).map(Volume::db);
        assert_eq!(db("--"), Some(-80.0));
        assert_eq!(db("00"), Some(-80.0));
        assert_eq!(db("0"), Some(-80.0));
        assert_eq!(db("43"), Some(-37.0));
        assert_eq!(db("435"), Some(-36.5));
        assert_eq!(db("430"), Some(-37.0));
        assert_eq!(db(" 50 "), Some(-30.0));
        assert_eq!(db("98"), Some(18.0));
        assert_eq!(db("985"), Some(18.0));
    }

    #[test]
    fn telnet_clamps_and_rejects() {
        assert_eq!(Volume::from_telnet("99"), Some(Volume::MIN));
        assert_eq!(Volume::from_telnet("990"), Some(Volume::MIN));
        assert_eq!(Volume::from_telnet("995"), Some(Volume::MIN));
        assert_eq!(Volume::from_telnet("991"), None);
        assert_eq!(Volume::from_telnet_clamped("99"), Some(Volume::MAX));
        assert_eq!(Volume::from_telnet_clamped("995"), Some(Volume::MAX));
        assert_eq!(Volume::from_telnet(""), None);
        assert_eq!(Volume::from_telnet("MAX"), None);
        assert_eq!(Volume::from_telnet("4350"), None);
        assert_eq!(Volume::from_telnet("431"), None);
        assert_eq!(Volume::from_telnet("-1"), None);
        assert_eq!(Volume::from_telnet("4a"), None);
    }

    #[test]
    fn telnet_round_trip() {
        for steps in 0..=196 {
            let volume = Volume::from_half_db_above_min(steps).unwrap();
            assert_eq!(Volume::from_telnet(&volume.to_string()), Some(volume));
        }
    }

    #[test]
    fn from_db_rounds_and_checks_range() {
        assert_eq!(Volume::from_db_rounded(-36.7).map(Volume::db), Some(-36.5));
        assert_eq!(Volume::from_db_rounded(-36.8).map(Volume::db), Some(-37.0));
        assert_eq!(Volume::from_db_rounded(-80.1).map(Volume::db), Some(-80.0));
        assert_eq!(Volume::from_db_rounded(-80.3), None);
        assert_eq!(Volume::from_db_rounded(18.2).map(Volume::db), Some(18.0));
        assert_eq!(Volume::from_db_rounded(18.3), None);
        assert_eq!(Volume::from_db_rounded(f32::NAN), None);
        assert_eq!(Volume::from_half_db_above_min(197), None);
    }

    proptest::proptest! {
        #[test]
        fn from_telnet_never_panics(text in "\\PC{0,8}") {
            let _ = Volume::from_telnet(&text);
        }

        #[test]
        fn from_db_never_panics(db in proptest::prelude::any::<f32>()) {
            if let Some(volume) = Volume::from_db_rounded(db) {
                proptest::prop_assert!((-80.0..=18.0).contains(&volume.db()));
            }
        }
    }
}
