//! Commands that a client sends on telnet.

use core::fmt::Write as _;

use crate::error::Error;
use crate::volume::Volume;
use crate::zone::Zone;

/// A telnet command.
///
/// Commands that depend on the zone are encoded with the zone given to
/// [`Command::write_line`]. The receiver answers a command with the same
/// events as for any other change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command<'a> {
    /// Turns the zone on.
    PowerOn,
    /// Puts the zone in standby.
    PowerStandby,
    /// Turns the whole receiver on.
    DevicePowerOn,
    /// Puts the whole receiver in standby.
    DevicePowerStandby,
    /// Raises the volume by one step.
    VolumeUp,
    /// Lowers the volume by one step.
    VolumeDown,
    /// Sets the volume.
    SetVolume(Volume),
    /// Mutes (`true`) or unmutes (`false`).
    Mute(bool),
    /// Selects a source by its telnet name, for example `GAME`.
    SelectSource(&'a str),
    /// Selects a sound mode by name, for example `MOVIE`.
    SelectSoundMode(&'a str),
    /// Turns the all zone stereo mode on (`true`) or off (`false`).
    AllZoneStereo(bool),
    /// Plays.
    Play,
    /// Pauses.
    Pause,
    /// Stops.
    Stop,
    /// Moves the cursor up in the menu.
    CursorUp,
    /// Moves the cursor down in the menu.
    CursorDown,
    /// Moves the cursor left in the menu.
    CursorLeft,
    /// Moves the cursor right in the menu.
    CursorRight,
    /// Confirms in the menu.
    CursorEnter,
    /// Goes back in the menu.
    Back,
    /// Shows information.
    Info,
    /// Shows options.
    Options,
    /// Opens (`true`) or closes (`false`) the setup menu.
    SetupMenu(bool),
    /// Sends text as it is, for example a query like `PW?`.
    Raw(&'a str),
}

impl Command<'_> {
    /// Appends the command and the line end to `out`.
    ///
    /// `out` is not changed if this returns an error.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommandText`] if text in the command is empty
    /// or has a control character.
    pub fn write_line(&self, zone: Zone, out: &mut String) -> Result<(), Error> {
        let start = out.len();
        let result = self.write(zone, out);
        if result.is_err() {
            out.truncate(start);
        } else {
            out.push('\r');
        }
        result
    }

    fn write(&self, zone: Zone, out: &mut String) -> Result<(), Error> {
        let (main, zone2, zone3) = (zone == Zone::Main, zone == Zone::Zone2, zone == Zone::Zone3);
        let pick = |a: &'static str, b: &'static str, c: &'static str| {
            if main {
                a
            } else if zone2 {
                b
            } else {
                debug_assert!(zone3);
                c
            }
        };
        match *self {
            Self::PowerOn => out.push_str(pick("ZMON", "Z2ON", "Z3ON")),
            Self::PowerStandby => out.push_str(pick("ZMOFF", "Z2OFF", "Z3OFF")),
            Self::DevicePowerOn => out.push_str("PWON"),
            Self::DevicePowerStandby => out.push_str("PWSTANDBY"),
            Self::VolumeUp => out.push_str(pick("MVUP", "Z2UP", "Z3UP")),
            Self::VolumeDown => out.push_str(pick("MVDOWN", "Z2DOWN", "Z3DOWN")),
            Self::SetVolume(volume) => {
                out.push_str(pick("MV", "Z2", "Z3"));
                // Writing to a `String` cannot fail.
                let _ = write!(out, "{volume}");
            }
            Self::Mute(true) => out.push_str(pick("MUON", "Z2MUON", "Z3MUON")),
            Self::Mute(false) => out.push_str(pick("MUOFF", "Z2MUOFF", "Z3MUOFF")),
            Self::SelectSource(name) => {
                check_text(name)?;
                out.push_str(pick("SI", "Z2", "Z3"));
                out.push_str(name);
            }
            Self::SelectSoundMode(name) => {
                check_text(name)?;
                out.push_str("MS");
                out.push_str(name);
            }
            Self::AllZoneStereo(true) => out.push_str("MNZST ON"),
            Self::AllZoneStereo(false) => out.push_str("MNZST OFF"),
            Self::Play => out.push_str("NS9A"),
            Self::Pause => out.push_str("NS9B"),
            Self::Stop => out.push_str("NS9C"),
            Self::CursorUp => out.push_str("MNCUP"),
            Self::CursorDown => out.push_str("MNCDN"),
            Self::CursorLeft => out.push_str("MNCLT"),
            Self::CursorRight => out.push_str("MNCRT"),
            Self::CursorEnter => out.push_str("MNENT"),
            Self::Back => out.push_str("MNRTN"),
            Self::Info => out.push_str("MNINF"),
            Self::Options => out.push_str("MNOPT"),
            Self::SetupMenu(true) => out.push_str("MNMEN ON"),
            Self::SetupMenu(false) => out.push_str("MNMEN OFF"),
            Self::Raw(text) => {
                check_text(text)?;
                out.push_str(text);
            }
        }
        Ok(())
    }
}

fn check_text(text: &str) -> Result<(), Error> {
    if text.is_empty() || text.chars().any(char::is_control) {
        Err(Error::InvalidCommandText)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(command: Command<'_>, zone: Zone) -> String {
        let mut out = String::new();
        command.write_line(zone, &mut out).unwrap();
        out
    }

    fn volume(db: f32) -> Volume {
        Volume::from_db_rounded(db).unwrap()
    }

    #[test]
    fn zone_commands_follow_the_zone() {
        let cases: [(Command<'_>, [&str; 3]); 8] = [
            (Command::PowerOn, ["ZMON\r", "Z2ON\r", "Z3ON\r"]),
            (Command::PowerStandby, ["ZMOFF\r", "Z2OFF\r", "Z3OFF\r"]),
            (Command::VolumeUp, ["MVUP\r", "Z2UP\r", "Z3UP\r"]),
            (Command::VolumeDown, ["MVDOWN\r", "Z2DOWN\r", "Z3DOWN\r"]),
            (Command::Mute(true), ["MUON\r", "Z2MUON\r", "Z3MUON\r"]),
            (Command::Mute(false), ["MUOFF\r", "Z2MUOFF\r", "Z3MUOFF\r"]),
            (
                Command::SelectSource("GAME"),
                ["SIGAME\r", "Z2GAME\r", "Z3GAME\r"],
            ),
            (
                Command::SetVolume(volume(-37.0)),
                ["MV43\r", "Z243\r", "Z343\r"],
            ),
        ];
        for (command, expected) in cases {
            for (zone, expected) in Zone::ALL.into_iter().zip(expected) {
                assert_eq!(line(command, zone), expected, "{command:?} {zone:?}");
            }
        }
    }

    #[test]
    fn half_step_volume() {
        assert_eq!(
            line(Command::SetVolume(volume(-36.5)), Zone::Main),
            "MV435\r"
        );
        assert_eq!(
            line(Command::SetVolume(volume(-80.0)), Zone::Main),
            "MV00\r"
        );
        assert_eq!(
            line(Command::SetVolume(volume(18.0)), Zone::Zone2),
            "Z298\r"
        );
    }

    #[test]
    fn zone_independent_commands() {
        let cases = [
            (Command::DevicePowerOn, "PWON\r"),
            (Command::DevicePowerStandby, "PWSTANDBY\r"),
            (Command::SelectSoundMode("MOVIE"), "MSMOVIE\r"),
            (Command::AllZoneStereo(true), "MNZST ON\r"),
            (Command::AllZoneStereo(false), "MNZST OFF\r"),
            (Command::Play, "NS9A\r"),
            (Command::Pause, "NS9B\r"),
            (Command::Stop, "NS9C\r"),
            (Command::CursorUp, "MNCUP\r"),
            (Command::CursorDown, "MNCDN\r"),
            (Command::CursorLeft, "MNCLT\r"),
            (Command::CursorRight, "MNCRT\r"),
            (Command::CursorEnter, "MNENT\r"),
            (Command::Back, "MNRTN\r"),
            (Command::Info, "MNINF\r"),
            (Command::Options, "MNOPT\r"),
            (Command::SetupMenu(true), "MNMEN ON\r"),
            (Command::SetupMenu(false), "MNMEN OFF\r"),
            (Command::Raw("PSDYNEQ ?"), "PSDYNEQ ?\r"),
        ];
        for (command, expected) in cases {
            for zone in Zone::ALL {
                assert_eq!(line(command, zone), expected, "{command:?}");
            }
        }
    }

    #[test]
    fn appends_to_existing_text() {
        let mut out = String::from("PW?\r");
        Command::Mute(true)
            .write_line(Zone::Main, &mut out)
            .unwrap();
        assert_eq!(out, "PW?\rMUON\r");
    }

    #[test]
    fn rejects_empty_text_and_control_characters() {
        for text in ["", "GAME\r", "GAME\nMV99", "A\u{0}B", "\t"] {
            for command in [
                Command::SelectSource(text),
                Command::SelectSoundMode(text),
                Command::Raw(text),
            ] {
                let mut out = String::from("KEEP");
                let error = command.write_line(Zone::Main, &mut out).unwrap_err();
                assert_eq!(error, Error::InvalidCommandText, "{text:?}");
                assert!(error.is_invalid_input());
                assert_eq!(out, "KEEP");
            }
        }
    }

    #[test]
    fn accepts_text_with_spaces_and_symbols() {
        assert_eq!(
            line(Command::SelectSource("SAT/CBL"), Zone::Main),
            "SISAT/CBL\r"
        );
        assert_eq!(
            line(Command::SelectSource("USB DIRECT"), Zone::Main),
            "SIUSB DIRECT\r"
        );
        assert_eq!(
            line(Command::SelectSoundMode("DOLBY DIGITAL"), Zone::Main),
            "MSDOLBY DIGITAL\r"
        );
    }
}
