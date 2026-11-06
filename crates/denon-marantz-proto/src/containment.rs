//! Containment amount of Audyssey LFC.

use core::fmt;

/// Containment amount of Audyssey LFC, from 1 to 7.
///
/// The telnet code is two digits, for example `04`. [`Display`](fmt::Display)
/// writes the telnet code.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContainmentAmount(u8);

impl ContainmentAmount {
    /// Lowest amount, 1.
    pub const MIN: Self = Self(1);
    /// Highest amount, 7.
    pub const MAX: Self = Self(7);

    /// Creates an amount from 1 to 7.
    #[must_use]
    pub const fn new(amount: u8) -> Option<Self> {
        if amount >= Self::MIN.0 && amount <= Self::MAX.0 {
            Some(Self(amount))
        } else {
            None
        }
    }

    /// The amount, from 1 to 7.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }

    /// Parses the telnet code of an event: one or two digits, for example `04`.
    #[must_use]
    pub fn from_telnet(code: &str) -> Option<Self> {
        if code.is_empty() || code.len() > 2 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Self::new(code.parse().ok()?)
    }
}

impl fmt::Display for ContainmentAmount {
    /// Writes the telnet code.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}", self.0)
    }
}

impl fmt::Debug for ContainmentAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn range_and_codes() {
        assert_eq!(ContainmentAmount::new(0), None);
        assert_eq!(ContainmentAmount::new(1), Some(ContainmentAmount::MIN));
        assert_eq!(ContainmentAmount::new(7), Some(ContainmentAmount::MAX));
        assert_eq!(ContainmentAmount::new(8), None);
        assert_eq!(
            ContainmentAmount::from_telnet("04").map(ContainmentAmount::get),
            Some(4)
        );
        assert_eq!(
            ContainmentAmount::from_telnet("4").map(ContainmentAmount::get),
            Some(4)
        );
        for bad in ["", "00", "08", "004", "x", "+4", "-1"] {
            assert_eq!(ContainmentAmount::from_telnet(bad), None, "{bad:?}");
        }
        assert_eq!(ContainmentAmount::MIN.to_string(), "01");
        assert_eq!(ContainmentAmount::MAX.to_string(), "07");
    }
}
