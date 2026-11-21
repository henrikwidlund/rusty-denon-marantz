//! Bass and treble level.

use core::fmt;

/// Bass or treble level from -14 dB to +14 dB in steps of 1 dB.
///
/// Zones 2 and 3 take -10 dB to +10 dB on most receivers, and -14 dB to
/// +14 dB in steps of 2 dB on a few. The main zone only accepts levels from
/// -6 dB to +6 dB, see [`ToneLevel::MAIN_MIN`], [`ToneLevel::MAIN_MAX`] and
/// [`ToneLevel::is_main_range`].
///
/// The telnet code is the level plus 50, from `36` to `64`: `50` is 0 dB.
/// [`Display`](fmt::Display) writes the telnet code. `AppCommand` uses the
/// steps above -6 dB, from 0 to 12.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToneLevel(u8);

impl ToneLevel {
    /// Lowest level, -14 dB.
    pub const MIN: Self = Self(0);
    /// Level of 0 dB.
    pub const ZERO: Self = Self(14);
    /// Highest level, +14 dB.
    pub const MAX: Self = Self(28);
    /// Lowest level of the main zone, -6 dB.
    pub const MAIN_MIN: Self = Self(8);
    /// Highest level of the main zone, +6 dB.
    pub const MAIN_MAX: Self = Self(20);

    /// Lowest and highest level in dB.
    pub(crate) const MAX_DB: i8 = 14;
    /// Telnet code of the lowest level.
    const TELNET_MIN: u8 = 36;
    /// Steps of the lowest `AppCommand` level above the lowest level.
    const APP_OFFSET: u8 = 8;

    /// Creates a level from dB, from -14 to +14.
    #[must_use]
    pub fn from_db(db: i8) -> Option<Self> {
        let steps = u8::try_from(i16::from(db) + i16::from(Self::MAX_DB)).ok()?;
        (steps <= Self::MAX.0).then_some(Self(steps))
    }

    /// Level in dB.
    #[must_use]
    pub fn db(self) -> i8 {
        i8::try_from(self.0).unwrap_or(i8::MAX) - Self::MAX_DB
    }

    /// Returns true if the main zone accepts the level: from -6 dB to +6 dB.
    #[must_use]
    pub const fn is_main_range(self) -> bool {
        self.0 >= Self::MAIN_MIN.0 && self.0 <= Self::MAIN_MAX.0
    }

    /// Parses the telnet code of an event, for example `50`.
    #[must_use]
    pub fn from_telnet(code: &str) -> Option<Self> {
        let steps = parse_digits(code)?.checked_sub(Self::TELNET_MIN)?;
        (steps <= Self::MAX.0).then_some(Self(steps))
    }

    /// Parses the steps in an `AppCommand` answer, for example `6`. The steps
    /// are above -6 dB, from 0 to 12.
    #[must_use]
    pub fn from_answer(text: &str) -> Option<Self> {
        let steps = parse_digits(text)?;
        (steps <= 12).then_some(Self(steps + Self::APP_OFFSET))
    }
}

/// A number of one to three digits and nothing else.
fn parse_digits(text: &str) -> Option<u8> {
    let text = text.trim();
    if text.is_empty() || text.len() > 3 || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

impl fmt::Display for ToneLevel {
    /// Writes the telnet code.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0 + Self::TELNET_MIN)
    }
}

impl fmt::Debug for ToneLevel {
    /// Writes the level in dB.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:+} dB", self.db())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telnet_codes() {
        assert_eq!(ToneLevel::from_telnet("36"), Some(ToneLevel::MIN));
        assert_eq!(ToneLevel::from_telnet("44"), Some(ToneLevel::MAIN_MIN));
        assert_eq!(ToneLevel::from_telnet("50"), Some(ToneLevel::ZERO));
        assert_eq!(ToneLevel::from_telnet("56"), Some(ToneLevel::MAIN_MAX));
        assert_eq!(ToneLevel::from_telnet("64"), Some(ToneLevel::MAX));
        for bad in ["", "35", "65", "5", "abc", "+50", "0050", "5 0"] {
            assert_eq!(ToneLevel::from_telnet(bad), None, "{bad:?}");
        }
        assert_eq!(ToneLevel::MIN.to_string(), "36");
        assert_eq!(ToneLevel::ZERO.to_string(), "50");
        assert_eq!(ToneLevel::MAX.to_string(), "64");
    }

    #[test]
    fn answers_and_db() {
        assert_eq!(ToneLevel::from_answer("0"), Some(ToneLevel::MAIN_MIN));
        assert_eq!(ToneLevel::from_answer("6"), Some(ToneLevel::ZERO));
        assert_eq!(ToneLevel::from_answer("12"), Some(ToneLevel::MAIN_MAX));
        assert_eq!(ToneLevel::from_answer("13"), None);
        assert_eq!(ToneLevel::from_answer("-1"), None);
        assert_eq!(ToneLevel::from_db(-14), Some(ToneLevel::MIN));
        assert_eq!(ToneLevel::from_db(-6), Some(ToneLevel::MAIN_MIN));
        assert_eq!(ToneLevel::from_db(0), Some(ToneLevel::ZERO));
        assert_eq!(ToneLevel::from_db(6), Some(ToneLevel::MAIN_MAX));
        assert_eq!(ToneLevel::from_db(14), Some(ToneLevel::MAX));
        assert_eq!(ToneLevel::from_db(15), None);
        assert_eq!(ToneLevel::from_db(-15), None);
        assert_eq!(ToneLevel::from_db(i8::MIN), None);
        assert_eq!(ToneLevel::from_db(i8::MAX), None);
        for db in -14..=14 {
            let level = ToneLevel::from_db(db).unwrap();
            assert_eq!(level.db(), db);
            assert_eq!(ToneLevel::from_telnet(&level.to_string()), Some(level));
            assert_eq!(level.is_main_range(), (-6..=6).contains(&db), "{db}");
        }
        assert_eq!(format!("{:?}", ToneLevel::MAX), "+14 dB");
        assert_eq!(format!("{:?}", ToneLevel::MIN), "-14 dB");
    }
}
