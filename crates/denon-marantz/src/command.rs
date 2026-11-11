//! Commands as text, for telnet and for HTTP.

use std::fmt::Write as _;

use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{Brand, Setting, Zone};

use crate::error::Error;

/// The page that runs a command from the text of the query part.
const COMMAND_PATH: &str = "/goform/formiPhoneAppDirect.xml?";

/// Encodes a command for a zone of a receiver of this brand. The line ends
/// with the carriage return of the telnet protocol.
pub(crate) fn encode(brand: Brand, zone: Zone, command: &Command<'_>) -> Result<String, Error> {
    let mut line = String::new();
    command
        .write_line(brand, zone, &mut line)
        .map_err(Error::from_command)?;
    Ok(line)
}

/// Returns true if the line asks for a value. The receiver does not answer
/// that over HTTP.
pub(crate) fn is_query(line: &str) -> bool {
    line.trim_end().ends_with('?')
}

/// The path and query that run the line over HTTP.
pub(crate) fn http_path(line: &str) -> String {
    let text = line.trim_end();
    let mut path = String::with_capacity(COMMAND_PATH.len() + text.len() + 8);
    path.push_str(COMMAND_PATH);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b':') {
            path.push(char::from(byte));
        } else {
            let _ = write!(path, "%{byte:02X}");
        }
    }
    path
}

/// What a command changes that the receiver reports over HTTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reads {
    Power,
    Volume,
    Source,
    SoundMode,
    Device,
}

impl Reads {
    /// The values that an HTTP read after the command can show. `None` if the
    /// read of the zones does not have what the command changes.
    pub(crate) const fn after(command: &Command<'_>) -> Option<Self> {
        match command {
            Command::PowerOn
            | Command::PowerStandby
            | Command::DevicePowerOn
            | Command::DevicePowerStandby => Some(Self::Power),
            Command::VolumeUp | Command::VolumeDown | Command::SetVolume(_) | Command::Mute(_) => {
                Some(Self::Volume)
            }
            Command::SelectSource(_) => Some(Self::Source),
            Command::SelectSoundMode(_) => Some(Self::SoundMode),
            Command::SetDeviceSetting(_) => Some(Self::Device),
            _ => None,
        }
    }

    pub(crate) const fn bit(self) -> u8 {
        1 << self as u8
    }

    /// The values that the receiver shows late: power, the source and the
    /// sound mode.
    pub(crate) const SLOW: u8 = Self::Power.bit() | Self::Source.bit() | Self::SoundMode.bit();

    /// The bit of the values that a change of this setting shows. 0 if the
    /// setting is not one of the late values.
    pub(crate) const fn of_setting(setting: Setting) -> u8 {
        match setting {
            Setting::Power => Self::Power.bit(),
            Setting::Source => Self::Source.bit(),
            Setting::SoundMode => Self::SoundMode.bit(),
            _ => 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_has_the_text_of_the_command() {
        assert_eq!(http_path("PWON\r"), "/goform/formiPhoneAppDirect.xml?PWON");
        assert_eq!(
            http_path("MSDOLBY DIGITAL\r"),
            "/goform/formiPhoneAppDirect.xml?MSDOLBY%20DIGITAL"
        );
        assert_eq!(
            http_path("PSMULTEQ:BYP.LR\r"),
            "/goform/formiPhoneAppDirect.xml?PSMULTEQ:BYP.LR"
        );
        // Characters with a meaning in a URL, and text that is not ASCII.
        assert_eq!(
            http_path("NAME a&b=c#d%e+f é\r"),
            "/goform/formiPhoneAppDirect.xml?NAME%20a%26b%3Dc%23d%25e%2Bf%20%C3%A9"
        );
    }

    #[test]
    fn a_query_is_found() {
        assert!(is_query("PW?\r"));
        assert!(is_query("DIM ?\r"));
        assert!(!is_query("PWON\r"));
    }

    #[test]
    fn commands_say_what_to_read_afterwards() {
        assert_eq!(Reads::after(&Command::PowerOn), Some(Reads::Power));
        assert_eq!(Reads::after(&Command::VolumeUp), Some(Reads::Volume));
        assert_eq!(Reads::after(&Command::Mute(true)), Some(Reads::Volume));
        assert_eq!(
            Reads::after(&Command::SelectSource("GAME")),
            Some(Reads::Source)
        );
        assert_eq!(
            Reads::after(&Command::SelectSoundMode("MOVIE")),
            Some(Reads::SoundMode)
        );
        assert_eq!(Reads::after(&Command::BassUp), None);
        let bits: Vec<u8> = [
            Reads::Power,
            Reads::Volume,
            Reads::Source,
            Reads::SoundMode,
            Reads::Device,
        ]
        .iter()
        .map(|reads| reads.bit())
        .collect();
        assert_eq!(bits, [1, 2, 4, 8, 16]);
    }
}
