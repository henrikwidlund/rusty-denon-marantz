//! Receiver zones and receiver generations.

/// Zone of a receiver.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Zone {
    /// Main zone.
    Main,
    /// Zone 2.
    Zone2,
    /// Zone 3.
    Zone3,
}

impl Zone {
    /// All zones.
    pub const ALL: [Self; 3] = [Self::Main, Self::Zone2, Self::Zone3];

    /// Zone name used in AppCommand XML and as a display name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Main => "Main",
            Self::Zone2 => "Zone2",
            Self::Zone3 => "Zone3",
        }
    }

    /// Position of the zone in tables that have one entry for each zone.
    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Main => 0,
            Self::Zone2 => 1,
            Self::Zone3 => 2,
        }
    }

    /// Parses a zone name.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|zone| zone.name() == name)
    }
}

/// Receiver generation. It decides the HTTP ports and the description URL.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ReceiverType {
    /// Older receiver without AVR-X firmware.
    Avr,
    /// AVR-X receiver.
    AvrX,
    /// AVR-X receiver from 2016 that uses port 8080.
    AvrX2016,
}

impl ReceiverType {
    /// All receiver types.
    pub const ALL: [Self; 3] = [Self::Avr, Self::AvrX, Self::AvrX2016];

    /// Short name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Avr => "avr",
            Self::AvrX => "avr-x",
            Self::AvrX2016 => "avr-x-2016",
        }
    }

    /// HTTP port for control and status requests.
    #[must_use]
    pub const fn port(self) -> u16 {
        match self {
            Self::Avr | Self::AvrX => 80,
            Self::AvrX2016 => 8080,
        }
    }

    /// HTTP port of the UPnP description.
    #[must_use]
    pub const fn description_port(self) -> u16 {
        match self {
            Self::Avr | Self::AvrX => 8080,
            Self::AvrX2016 => 60006,
        }
    }

    /// Path of the UPnP description.
    #[must_use]
    pub const fn description_path(self) -> &'static str {
        match self {
            Self::Avr | Self::AvrX => "/description.xml",
            Self::AvrX2016 => "/upnp/desc/aios_device/aios_device.xml",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_names_round_trip() {
        for zone in Zone::ALL {
            assert_eq!(Zone::from_name(zone.name()), Some(zone));
        }
        assert_eq!(Zone::from_name("main"), None);
    }

    #[test]
    fn receiver_type_values() {
        assert_eq!(ReceiverType::AvrX2016.port(), 8080);
        assert_eq!(ReceiverType::AvrX2016.description_port(), 60006);
        assert_eq!(ReceiverType::Avr.description_path(), "/description.xml");
    }
}
