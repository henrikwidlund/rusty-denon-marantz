//! Bass and treble level.

use core::fmt;

/// Bass or treble level from -6 dB to +6 dB in steps of 1 dB.
///
/// The telnet code is the level plus 50, from `44` to `56`: `50` is 0 dB.
/// [`Display`](fmt::Display) writes the telnet code. `AppCommand` uses the
/// steps above the lowest level, from 0 to 12.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ToneLevel(u8);

impl ToneLevel {
    /// Lowest level, -6 dB.
    pub const MIN: Self = Self(0);
    /// Level of 0 dB.
    pub const ZERO: Self = Self(6);
    /// Highest level, +6 dB.
    pub const MAX: Self = Self(12);

    /// Telnet code of the lowest level.
    const TELNET_MIN: u8 = 44;

    /// Creates a level from steps of 1 dB above -6 dB, from 0 to 12.
    #[must_use]
    pub const fn from_steps(steps: u8) -> Option<Self> {
        if steps <= Self::MAX.0 {
            Some(Self(steps))
        } else {
            None
        }
    }

    /// Creates a level from dB, from -6 to +6.
    #[must_use]
    pub fn from_db(db: i8) -> Option<Self> {
        Self::from_steps(u8::try_from(i16::from(db) + 6).ok()?)
    }

    /// Level in dB.
    #[must_use]
    pub fn db(self) -> i8 {
        i8::try_from(self.0).unwrap_or(i8::MAX) - 6
    }

    /// Level in steps of 1 dB above -6 dB.
    #[must_use]
    pub const fn steps(self) -> u8 {
        self.0
    }

    /// Parses the telnet code of an event, for example `50`.
    #[must_use]
    pub fn from_telnet(code: &str) -> Option<Self> {
        Self::from_steps(parse_digits(code)?.checked_sub(Self::TELNET_MIN)?)
    }

    /// Parses the steps in an `AppCommand` answer, for example `6`.
    #[must_use]
    pub fn from_answer(text: &str) -> Option<Self> {
        Self::from_steps(parse_digits(text)?)
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
        assert_eq!(ToneLevel::from_telnet("44"), Some(ToneLevel::MIN));
        assert_eq!(ToneLevel::from_telnet("50"), Some(ToneLevel::ZERO));
        assert_eq!(ToneLevel::from_telnet("56"), Some(ToneLevel::MAX));
        for bad in ["", "43", "57", "5", "abc", "+50", "0050", "5 0"] {
            assert_eq!(ToneLevel::from_telnet(bad), None, "{bad:?}");
        }
        assert_eq!(ToneLevel::MIN.to_string(), "44");
        assert_eq!(ToneLevel::ZERO.to_string(), "50");
        assert_eq!(ToneLevel::MAX.to_string(), "56");
    }

    #[test]
    fn answers_and_db() {
        assert_eq!(ToneLevel::from_answer("0"), Some(ToneLevel::MIN));
        assert_eq!(ToneLevel::from_answer("12"), Some(ToneLevel::MAX));
        assert_eq!(ToneLevel::from_answer("13"), None);
        assert_eq!(ToneLevel::from_answer("-1"), None);
        assert_eq!(ToneLevel::from_db(-6), Some(ToneLevel::MIN));
        assert_eq!(ToneLevel::from_db(0), Some(ToneLevel::ZERO));
        assert_eq!(ToneLevel::from_db(6), Some(ToneLevel::MAX));
        assert_eq!(ToneLevel::from_db(7), None);
        assert_eq!(ToneLevel::from_db(-7), None);
        assert_eq!(ToneLevel::from_db(i8::MIN), None);
        assert_eq!(ToneLevel::from_db(i8::MAX), None);
        for steps in 0..=12 {
            let level = ToneLevel::from_steps(steps).unwrap();
            assert_eq!(ToneLevel::from_db(level.db()), Some(level));
            assert_eq!(ToneLevel::from_telnet(&level.to_string()), Some(level));
        }
        assert_eq!(format!("{:?}", ToneLevel::MAX), "+6 dB");
        assert_eq!(format!("{:?}", ToneLevel::MIN), "-6 dB");
    }
}
