//! Receiver brand.

/// Brand of a receiver. Some telnet commands and queries differ by brand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Brand {
    /// Denon.
    Denon,
    /// Marantz.
    Marantz,
}

impl Brand {
    /// Finds the brand from the manufacturer name in the device information.
    ///
    /// A missing or empty name gives [`Brand::Denon`]. Any other name that does
    /// not contain "denon" gives [`Brand::Marantz`].
    #[must_use]
    pub fn from_manufacturer(manufacturer: Option<&str>) -> Self {
        let Some(name) = manufacturer.filter(|name| !name.is_empty()) else {
            return Self::Denon;
        };
        let contains_denon = name
            .as_bytes()
            .windows(5)
            .any(|window| window.eq_ignore_ascii_case(b"denon"));
        if contains_denon {
            Self::Denon
        } else {
            Self::Marantz
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_from_manufacturer() {
        assert_eq!(Brand::from_manufacturer(None), Brand::Denon);
        assert_eq!(Brand::from_manufacturer(Some("")), Brand::Denon);
        assert_eq!(Brand::from_manufacturer(Some("Denon")), Brand::Denon);
        assert_eq!(Brand::from_manufacturer(Some("DENON Ltd.")), Brand::Denon);
        assert_eq!(Brand::from_manufacturer(Some("Marantz")), Brand::Marantz);
        assert_eq!(Brand::from_manufacturer(Some("Den")), Brand::Marantz);
    }
}
