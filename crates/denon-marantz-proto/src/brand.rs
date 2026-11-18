//! Receiver brand.

/// Brand of a receiver. Some telnet commands and queries differ by brand.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Brand {
    /// Denon.
    Denon,
    /// Marantz.
    Marantz,
}

impl Brand {
    /// Name of the brand.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Denon => "Denon",
            Self::Marantz => "Marantz",
        }
    }

    /// Finds the brand from `BrandCode` in `Deviceinfo.xml`: `0` is Denon,
    /// `1` is Marantz. Other values give `None`.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        match code.trim() {
            "0" => Some(Self::Denon),
            "1" => Some(Self::Marantz),
            _ => None,
        }
    }

    /// Finds the brand from a model text, for example `Marantz SR6015`.
    /// `None` if the text names no brand.
    #[must_use]
    pub fn from_model(model: &str) -> Option<Self> {
        if contains_ignore_case(model, b"marantz") {
            Some(Self::Marantz)
        } else if contains_ignore_case(model, b"denon") {
            Some(Self::Denon)
        } else {
            None
        }
    }

    /// Finds the brand from the manufacturer name in the device information.
    ///
    /// A missing or empty name gives [`Brand::Denon`]. Any other name that does
    /// not contain "denon" gives [`Brand::Marantz`].
    #[must_use]
    pub fn from_manufacturer(manufacturer: Option<&str>) -> Self {
        let Some(name) = manufacturer.filter(|name| !name.is_empty()) else {
            return Self::Denon;
        };
        if contains_ignore_case(name, b"denon") {
            Self::Denon
        } else {
            Self::Marantz
        }
    }
}

/// Returns true if the text contains the ASCII word, ignoring case.
fn contains_ignore_case(text: &str, word: &[u8]) -> bool {
    text.as_bytes()
        .windows(word.len())
        .any(|window| window.eq_ignore_ascii_case(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brand_names() {
        assert_eq!(Brand::Denon.name(), "Denon");
        assert_eq!(Brand::Marantz.name(), "Marantz");
    }

    #[test]
    fn brand_from_code() {
        assert_eq!(Brand::from_code("0"), Some(Brand::Denon));
        assert_eq!(Brand::from_code(" 1 "), Some(Brand::Marantz));
        assert_eq!(Brand::from_code("2"), None);
        assert_eq!(Brand::from_code(""), None);
    }

    #[test]
    fn brand_from_model() {
        assert_eq!(Brand::from_model("MARANTZ SR6015"), Some(Brand::Marantz));
        assert_eq!(Brand::from_model("Denon AVR-X3700H"), Some(Brand::Denon));
        assert_eq!(Brand::from_model("AVR-X3700H"), None);
        assert_eq!(Brand::from_model(""), None);
    }

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
