//! Remote keys that have no telnet command.
//!
//! The receiver accepts these keys as text on telnet, the same text as the
//! infrared remote sends. Functions that have a telnet command are not here.

use core::fmt::Write as _;

use crate::brand::Brand;
use crate::error::Error;
use crate::zone::Zone;

/// A remote key that has no telnet command.
///
/// A key that the receiver of a brand or a zone does not have gives
/// [`Error::UnsupportedKey`] when it is sent. Marantz keys work on the main
/// zone only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RemoteKey {
    /// Plays the next track of the network or USB source.
    NextTrack,
    /// Plays the previous track of the network or USB source.
    PreviousTrack,
    /// Turns repeat off.
    RepeatOff,
    /// Repeats the current track.
    RepeatOne,
    /// Repeats all tracks.
    RepeatAll,
    /// Turns random play on.
    RandomOn,
    /// Turns random play off.
    RandomOff,
    /// Goes to the previous page of a list or a menu.
    PageUp,
    /// Goes to the next page of a list or a menu.
    PageDown,
    /// Rewinds. On Marantz the key switches the rewind mode on and off.
    Rewind,
    /// Winds forward. On Marantz the key switches the wind mode on and off.
    FastForward,
    /// Switches Dolby Atmos on and off. Main zone only.
    /// [`SoundSettingValue::DolbyAtmos`](crate::SoundSettingValue::DolbyAtmos)
    /// sets it.
    DolbyAtmosToggle,
    /// Turns HDMI control (CEC) on. Main zone only.
    HdmiCecOn,
    /// Turns HDMI control (CEC) off. Main zone only.
    HdmiCecOff,
    /// A digit from 0 to 9. Denon only.
    Digit(u8),
    /// Sets the volume to 0 dB. Denon only.
    MasterVolume0Db,
    /// Sets the volume to -20 dB. Denon only.
    MasterVolumeMinus20Db,
    /// Sets the volume to -40 dB. Denon only.
    MasterVolumeMinus40Db,
}

/// Which keys of a Denon receiver a key number belongs to.
#[derive(Clone, Copy)]
enum DenonScope {
    /// The number is the same for each zone, with the zone as category.
    Zone,
    /// Main zone only.
    Main,
    /// Playback of network and USB sources, for all zones.
    Playback,
}

impl DenonScope {
    /// Category digit in the key text, or `None` if the zone has no such key.
    const fn category(self, zone: Zone) -> Option<char> {
        match (self, zone) {
            (Self::Playback, _) => Some('7'),
            (Self::Zone | Self::Main, Zone::Main) => Some('1'),
            (Self::Zone, Zone::Zone2) => Some('3'),
            (Self::Zone, Zone::Zone3) => Some('5'),
            (Self::Main, _) => None,
        }
    }
}

impl RemoteKey {
    /// Appends the key text for a receiver of `brand` to `out`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedKey`] if the brand or the zone has no such
    /// key, and [`Error::ValueOutOfRange`] for a digit above 9. `out` is not
    /// changed in that case.
    pub(crate) fn write(self, brand: Brand, zone: Zone, out: &mut String) -> Result<(), Error> {
        match brand {
            Brand::Denon => self.write_denon(zone, out),
            Brand::Marantz => self.write_marantz(zone, out),
        }
    }

    fn write_denon(self, zone: Zone, out: &mut String) -> Result<(), Error> {
        let (scope, number) = match self {
            Self::NextTrack => (DenonScope::Playback, 776),
            Self::PreviousTrack => (DenonScope::Playback, 777),
            Self::RepeatOne => (DenonScope::Playback, 778),
            Self::RepeatOff => (DenonScope::Playback, 779),
            Self::RandomOn => (DenonScope::Playback, 780),
            Self::RandomOff => (DenonScope::Playback, 781),
            Self::RepeatAll => (DenonScope::Playback, 785),
            Self::PageUp => (DenonScope::Zone, 945),
            Self::PageDown => (DenonScope::Zone, 946),
            Self::Rewind => (DenonScope::Zone, 976),
            Self::FastForward => (DenonScope::Zone, 979),
            Self::DolbyAtmosToggle => (DenonScope::Main, 348),
            Self::HdmiCecOn => (DenonScope::Main, 826),
            Self::HdmiCecOff => (DenonScope::Main, 827),
            Self::Digit(0) => (DenonScope::Zone, 25),
            Self::Digit(digit @ 1..=9) => (DenonScope::Zone, 15 + u16::from(digit)),
            Self::Digit(_) => return Err(Error::ValueOutOfRange { what: "digit" }),
            Self::MasterVolume0Db => (DenonScope::Zone, 372),
            Self::MasterVolumeMinus20Db => (DenonScope::Zone, 373),
            Self::MasterVolumeMinus40Db => (DenonScope::Zone, 374),
        };
        let category = scope.category(zone).ok_or(Error::UnsupportedKey)?;
        // Writing to a `String` cannot fail.
        let _ = write!(out, "RCKSK04{category}{number:04}");
        Ok(())
    }

    fn write_marantz(self, zone: Zone, out: &mut String) -> Result<(), Error> {
        if zone != Zone::Main {
            return Err(Error::UnsupportedKey);
        }
        let text = match self {
            Self::NextTrack => "RCRC52703202",
            Self::PreviousTrack => "RCRC52703302",
            Self::RepeatOff => "RCRC52702902",
            Self::RepeatOne => "RCRC52702903",
            Self::RepeatAll => "RCRC52702904",
            Self::RandomOn => "RCRC52702803",
            Self::RandomOff => "RCRC52702802",
            Self::PageUp => "RCRC52708105",
            Self::PageDown => "RCRC52708005",
            Self::Rewind => "RCRC52705003",
            Self::FastForward => "RCRC52705203",
            Self::DolbyAtmosToggle => "RCRC51606425",
            Self::HdmiCecOn => "RCRC51608408",
            Self::HdmiCecOff => "RCRC51608409",
            Self::Digit(_)
            | Self::MasterVolume0Db
            | Self::MasterVolumeMinus20Db
            | Self::MasterVolumeMinus40Db => return Err(Error::UnsupportedKey),
        };
        out.push_str(text);
        Ok(())
    }
}

/// Appends the text of a Denon key code. The zone gives the category.
///
/// # Errors
///
/// Returns [`Error::ValueOutOfRange`] if the code has more than 4 digits, and
/// [`Error::UnsupportedKey`] for a Marantz receiver.
pub(crate) fn write_denon_code(
    brand: Brand,
    zone: Zone,
    code: u16,
    out: &mut String,
) -> Result<(), Error> {
    if brand != Brand::Denon {
        return Err(Error::UnsupportedKey);
    }
    if code > 9999 {
        return Err(Error::ValueOutOfRange { what: "key code" });
    }
    let category = DenonScope::Zone
        .category(zone)
        .ok_or(Error::UnsupportedKey)?;
    // Writing to a `String` cannot fail.
    let _ = write!(out, "RCKSK04{category}{code:04}");
    Ok(())
}

/// Appends the text of a Marantz key code. `digits` is the part after `RCRC5`.
///
/// # Errors
///
/// Returns [`Error::InvalidCommandText`] if `digits` is empty or has a
/// character that is not a digit, and [`Error::UnsupportedKey`] for a Denon
/// receiver.
pub(crate) fn write_marantz_code(
    brand: Brand,
    digits: &str,
    out: &mut String,
) -> Result<(), Error> {
    if brand != Brand::Marantz {
        return Err(Error::UnsupportedKey);
    }
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(Error::InvalidCommandText);
    }
    out.push_str("RCRC5");
    out.push_str(digits);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(key: RemoteKey, brand: Brand, zone: Zone) -> Result<String, Error> {
        let mut out = String::new();
        key.write(brand, zone, &mut out).map(|()| out)
    }

    #[test]
    fn denon_playback_keys_ignore_the_zone() {
        for zone in [Zone::Main, Zone::Zone2, Zone::Zone3] {
            assert_eq!(
                text(RemoteKey::NextTrack, Brand::Denon, zone).unwrap(),
                "RCKSK0470776"
            );
            assert_eq!(
                text(RemoteKey::RepeatAll, Brand::Denon, zone).unwrap(),
                "RCKSK0470785"
            );
        }
    }

    #[test]
    fn denon_zone_keys_use_the_zone_category() {
        let page_up = |zone| text(RemoteKey::PageUp, Brand::Denon, zone).unwrap();
        assert_eq!(page_up(Zone::Main), "RCKSK0410945");
        assert_eq!(page_up(Zone::Zone2), "RCKSK0430945");
        assert_eq!(page_up(Zone::Zone3), "RCKSK0450945");
        assert_eq!(
            text(RemoteKey::Rewind, Brand::Denon, Zone::Zone2).unwrap(),
            "RCKSK0430976"
        );
        assert_eq!(
            text(RemoteKey::MasterVolumeMinus20Db, Brand::Denon, Zone::Zone3).unwrap(),
            "RCKSK0450373"
        );
    }

    #[test]
    fn denon_main_only_keys_fail_in_other_zones() {
        assert_eq!(
            text(RemoteKey::HdmiCecOn, Brand::Denon, Zone::Main).unwrap(),
            "RCKSK0410826"
        );
        assert_eq!(
            text(RemoteKey::DolbyAtmosToggle, Brand::Denon, Zone::Zone2),
            Err(Error::UnsupportedKey)
        );
    }

    #[test]
    fn denon_digits() {
        let digit = |d| text(RemoteKey::Digit(d), Brand::Denon, Zone::Main);
        assert_eq!(digit(1).unwrap(), "RCKSK0410016");
        assert_eq!(digit(9).unwrap(), "RCKSK0410024");
        assert_eq!(digit(0).unwrap(), "RCKSK0410025");
        assert_eq!(digit(10), Err(Error::ValueOutOfRange { what: "digit" }));
    }

    #[test]
    fn marantz_keys_work_on_the_main_zone() {
        assert_eq!(
            text(RemoteKey::PageUp, Brand::Marantz, Zone::Main).unwrap(),
            "RCRC52708105"
        );
        assert_eq!(
            text(RemoteKey::HdmiCecOff, Brand::Marantz, Zone::Main).unwrap(),
            "RCRC51608409"
        );
        assert_eq!(
            text(RemoteKey::PageUp, Brand::Marantz, Zone::Zone2),
            Err(Error::UnsupportedKey)
        );
    }

    #[test]
    fn marantz_has_no_digits_or_volume_presets() {
        for key in [
            RemoteKey::Digit(1),
            RemoteKey::MasterVolume0Db,
            RemoteKey::MasterVolumeMinus20Db,
            RemoteKey::MasterVolumeMinus40Db,
        ] {
            assert_eq!(
                text(key, Brand::Marantz, Zone::Main),
                Err(Error::UnsupportedKey)
            );
        }
    }

    #[test]
    fn a_failed_key_leaves_the_text_unchanged() {
        let mut out = String::from("x");
        assert!(
            RemoteKey::Digit(1)
                .write(Brand::Marantz, Zone::Main, &mut out)
                .is_err()
        );
        assert_eq!(out, "x");
    }

    #[test]
    fn denon_code() {
        let code = |zone, code| {
            let mut out = String::new();
            write_denon_code(Brand::Denon, zone, code, &mut out).map(|()| out)
        };
        assert_eq!(code(Zone::Main, 945).unwrap(), "RCKSK0410945");
        assert_eq!(code(Zone::Zone3, 5).unwrap(), "RCKSK0450005");
        assert_eq!(
            code(Zone::Main, 10000),
            Err(Error::ValueOutOfRange { what: "key code" })
        );
        let mut out = String::new();
        assert_eq!(
            write_denon_code(Brand::Marantz, Zone::Main, 1, &mut out),
            Err(Error::UnsupportedKey)
        );
        assert_eq!(out, "");
    }

    #[test]
    fn marantz_code() {
        let code = |brand, digits| {
            let mut out = String::new();
            write_marantz_code(brand, digits, &mut out).map(|()| out)
        };
        assert_eq!(code(Brand::Marantz, "6012").unwrap(), "RCRC56012");
        assert_eq!(code(Brand::Marantz, "2708105").unwrap(), "RCRC52708105");
        assert_eq!(code(Brand::Marantz, ""), Err(Error::InvalidCommandText));
        assert_eq!(
            code(Brand::Marantz, "60 12"),
            Err(Error::InvalidCommandText)
        );
        assert_eq!(code(Brand::Denon, "6012"), Err(Error::UnsupportedKey));
    }
}
