//! Sleep timer.

use core::fmt;

/// Sleep timer of a zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sleep {
    /// No timer.
    Off,
    /// Minutes until standby, from 1 to 120.
    Minutes(u8),
}

impl Sleep {
    /// Longest timer in minutes.
    pub const MAX_MINUTES: u8 = 120;

    /// Creates a timer of 1 to 120 minutes.
    #[must_use]
    pub const fn minutes(minutes: u8) -> Option<Self> {
        if minutes >= 1 && minutes <= Self::MAX_MINUTES {
            Some(Self::Minutes(minutes))
        } else {
            None
        }
    }

    /// Parses a telnet value: `OFF` or one to three digits, for example `010`.
    #[must_use]
    pub fn from_telnet(code: &str) -> Option<Self> {
        if code == "OFF" {
            return Some(Self::Off);
        }
        if code.is_empty() || code.len() > 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Self::minutes(code.parse().ok()?)
    }
}

impl fmt::Display for Sleep {
    /// Writes the telnet value: `OFF` or three digits.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off => f.write_str("OFF"),
            Self::Minutes(minutes) => write!(f, "{minutes:03}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn telnet_known_answers() {
        assert_eq!(Sleep::from_telnet("OFF"), Some(Sleep::Off));
        assert_eq!(Sleep::from_telnet("010"), Some(Sleep::Minutes(10)));
        assert_eq!(Sleep::from_telnet("10"), Some(Sleep::Minutes(10)));
        assert_eq!(Sleep::from_telnet("120"), Some(Sleep::Minutes(120)));
        assert_eq!(Sleep::from_telnet("001"), Some(Sleep::Minutes(1)));
    }

    #[test]
    fn telnet_rejects_garbage() {
        for code in [
            "", "0", "000", "121", "999", "1000", "off", "-5", "1a", " 10",
        ] {
            assert_eq!(Sleep::from_telnet(code), None, "{code:?}");
        }
    }

    #[test]
    fn display_is_the_telnet_value() {
        assert_eq!(Sleep::Off.to_string(), "OFF");
        assert_eq!(Sleep::Minutes(5).to_string(), "005");
        assert_eq!(Sleep::Minutes(120).to_string(), "120");
        for minutes in 1..=120 {
            let sleep = Sleep::Minutes(minutes);
            assert_eq!(Sleep::from_telnet(&sleep.to_string()), Some(sleep));
        }
    }

    #[test]
    fn constructor_checks_range() {
        assert_eq!(Sleep::minutes(0), None);
        assert_eq!(Sleep::minutes(121), None);
        assert_eq!(Sleep::minutes(60), Some(Sleep::Minutes(60)));
    }
}
