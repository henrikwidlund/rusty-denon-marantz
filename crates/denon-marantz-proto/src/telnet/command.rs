//! Commands that a client sends on telnet.

use core::fmt::Write as _;

use crate::brand::Brand;
use crate::containment::ContainmentAmount;
use crate::error::Error;
use crate::remote_key::{RemoteKey, write_denon_code, write_marantz_code};
use crate::sound_settings::SoundSettingValue;
use crate::text::reject_control;
use crate::tone::ToneLevel;
use crate::values::{DiracFilter, DynamicVolume, InputMode, MultiEq, ReferenceLevelOffset};
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
    /// Turns tone control on (`true`) or off (`false`). Bass and treble work
    /// only when it is on. The receiver ignores this while Dynamic EQ is on,
    /// or with a Dirac Live filter such as Dirac ART.
    ToneControl(bool),
    /// Sets the bass level.
    SetBass(ToneLevel),
    /// Raises the bass by one step.
    BassUp,
    /// Lowers the bass by one step.
    BassDown,
    /// Sets the treble level.
    SetTreble(ToneLevel),
    /// Raises the treble by one step.
    TrebleUp,
    /// Lowers the treble by one step.
    TrebleDown,
    /// Turns Audyssey Dynamic EQ on (`true`) or off (`false`).
    DynamicEq(bool),
    /// Sets the Audyssey MultEQ mode.
    SetMultiEq(MultiEq),
    /// Sets the Audyssey reference level offset. The receiver accepts it only
    /// with Dynamic EQ on.
    SetReferenceLevelOffset(ReferenceLevelOffset),
    /// Sets the Audyssey Dynamic Volume.
    SetDynamicVolume(DynamicVolume),
    /// Turns Audyssey LFC on (`true`) or off (`false`).
    Lfc(bool),
    /// Sets the Audyssey LFC containment amount.
    SetContainmentAmount(ContainmentAmount),
    /// Raises the LFC containment amount by one step.
    ContainmentAmountUp,
    /// Lowers the LFC containment amount by one step.
    ContainmentAmountDown,
    /// Selects a Dirac Live filter.
    SetDiracFilter(DiracFilter),
    /// Turns Auto Lip Sync on (`true`) or off (`false`). The text depends on
    /// the brand.
    AutoLipSync(bool),
    /// Sets the audio input mode of the main zone.
    SetInputMode(InputMode),
    /// Sets a sound setting, for example the room size or the audio delay.
    SetSoundSetting(SoundSettingValue),
    /// Presses a remote key that has no telnet command. See [`RemoteKey`] for
    /// the errors.
    RemoteKey(RemoteKey),
    /// Presses a Denon key by its number, for example `945`. The zone gives
    /// the category. Returns [`Error::UnsupportedKey`] for Marantz.
    DenonKeyCode(u16),
    /// Presses a Marantz key by the digits after `RCRC5`, for example
    /// `2708105`. Returns [`Error::UnsupportedKey`] for Denon.
    MarantzKeyCode(&'a str),
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
    pub fn write_line(&self, brand: Brand, zone: Zone, out: &mut String) -> Result<(), Error> {
        let start = out.len();
        let result = self.write(brand, zone, out);
        if result.is_err() {
            out.truncate(start);
        } else {
            out.push('\r');
        }
        result
    }

    fn write(&self, brand: Brand, zone: Zone, out: &mut String) -> Result<(), Error> {
        match *self {
            Self::PowerOn => push_zone(out, zone, "ZM", "ON"),
            Self::PowerStandby => push_zone(out, zone, "ZM", "OFF"),
            Self::DevicePowerOn => out.push_str("PWON"),
            Self::DevicePowerStandby => out.push_str("PWSTANDBY"),
            Self::VolumeUp => push_zone(out, zone, "MV", "UP"),
            Self::VolumeDown => push_zone(out, zone, "MV", "DOWN"),
            Self::SetVolume(volume) => {
                out.push_str(zone_prefix(zone, "MV"));
                // Writing to a `String` cannot fail.
                let _ = write!(out, "{volume}");
            }
            Self::Mute(true) => push_zone(out, zone, "", "MUON"),
            Self::Mute(false) => push_zone(out, zone, "", "MUOFF"),
            Self::SelectSource(name) => {
                check_text(name)?;
                out.push_str(zone_prefix(zone, "SI"));
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
            Self::ToneControl(true) => out.push_str("PSTONE CTRL ON"),
            Self::ToneControl(false) => out.push_str("PSTONE CTRL OFF"),
            Self::SetBass(level) => {
                let _ = write!(out, "PSBAS {level}");
            }
            Self::BassUp => out.push_str("PSBAS UP"),
            Self::BassDown => out.push_str("PSBAS DOWN"),
            Self::SetTreble(level) => {
                let _ = write!(out, "PSTRE {level}");
            }
            Self::TrebleUp => out.push_str("PSTRE UP"),
            Self::TrebleDown => out.push_str("PSTRE DOWN"),
            Self::DynamicEq(true) => out.push_str("PSDYNEQ ON"),
            Self::DynamicEq(false) => out.push_str("PSDYNEQ OFF"),
            Self::SetMultiEq(mode) => {
                out.push_str("PSMULTEQ:");
                out.push_str(mode.telnet_code());
            }
            Self::SetReferenceLevelOffset(offset) => {
                out.push_str("PSREFLEV ");
                out.push_str(offset.telnet_code());
            }
            Self::SetDynamicVolume(volume) => {
                out.push_str("PSDYNVOL ");
                out.push_str(volume.telnet_code());
            }
            Self::Lfc(true) => out.push_str("PSLFC ON"),
            Self::Lfc(false) => out.push_str("PSLFC OFF"),
            Self::SetContainmentAmount(amount) => {
                let _ = write!(out, "PSCNTAMT {amount}");
            }
            Self::ContainmentAmountUp => out.push_str("PSCNTAMT UP"),
            Self::ContainmentAmountDown => out.push_str("PSCNTAMT DOWN"),
            Self::SetDiracFilter(filter) => {
                out.push_str("PSDIRAC ");
                out.push_str(filter.code());
            }
            Self::AutoLipSync(on) => {
                out.push_str(match brand {
                    Brand::Denon => "OPALSSET ",
                    Brand::Marantz => "SSHOSALS ",
                });
                out.push_str(if on { "ON" } else { "OFF" });
            }
            Self::SetInputMode(mode) => {
                out.push_str("SD");
                out.push_str(mode.code());
            }
            Self::SetSoundSetting(value) => value.write(out)?,
            Self::RemoteKey(key) => key.write(brand, zone, out)?,
            Self::DenonKeyCode(code) => write_denon_code(brand, zone, code, out)?,
            Self::MarantzKeyCode(digits) => write_marantz_code(brand, digits, out)?,
            Self::Raw(text) => {
                check_text(text)?;
                out.push_str(text);
            }
        }
        Ok(())
    }
}

/// Start of a command for a zone. Each kind of command has its own start for
/// the main zone. The other zones use `Z2` and `Z3`.
const fn zone_prefix(zone: Zone, main: &'static str) -> &'static str {
    match zone {
        Zone::Main => main,
        Zone::Zone2 => "Z2",
        Zone::Zone3 => "Z3",
    }
}

fn push_zone(out: &mut String, zone: Zone, main: &'static str, rest: &str) {
    out.push_str(zone_prefix(zone, main));
    out.push_str(rest);
}

fn check_text(text: &str) -> Result<(), Error> {
    if text.is_empty() {
        Err(Error::InvalidCommandText)
    } else {
        reject_control(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(command: Command<'_>, zone: Zone) -> String {
        let mut out = String::new();
        command.write_line(Brand::Denon, zone, &mut out).unwrap();
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
    fn tone_commands_ignore_the_zone() {
        let level = |db| ToneLevel::from_db(db).unwrap();
        let cases = [
            (Command::ToneControl(true), "PSTONE CTRL ON\r"),
            (Command::ToneControl(false), "PSTONE CTRL OFF\r"),
            (Command::SetBass(level(-6)), "PSBAS 44\r"),
            (Command::SetBass(level(0)), "PSBAS 50\r"),
            (Command::BassUp, "PSBAS UP\r"),
            (Command::BassDown, "PSBAS DOWN\r"),
            (Command::SetTreble(level(6)), "PSTRE 56\r"),
            (Command::TrebleUp, "PSTRE UP\r"),
            (Command::TrebleDown, "PSTRE DOWN\r"),
        ];
        for (command, expected) in cases {
            for zone in Zone::ALL {
                assert_eq!(line(command, zone), expected, "{command:?} {zone:?}");
            }
        }
    }

    #[test]
    fn audyssey_and_dirac_commands_ignore_the_zone() {
        let amount = |n| ContainmentAmount::new(n).unwrap();
        let cases = [
            (Command::DynamicEq(true), "PSDYNEQ ON\r"),
            (Command::DynamicEq(false), "PSDYNEQ OFF\r"),
            (Command::SetMultiEq(MultiEq::LrBypass), "PSMULTEQ:BYP.LR\r"),
            (
                Command::SetMultiEq(MultiEq::Reference),
                "PSMULTEQ:AUDYSSEY\r",
            ),
            (
                Command::SetReferenceLevelOffset(ReferenceLevelOffset::Db10),
                "PSREFLEV 10\r",
            ),
            (
                Command::SetDynamicVolume(DynamicVolume::Heavy),
                "PSDYNVOL HEV\r",
            ),
            (Command::Lfc(true), "PSLFC ON\r"),
            (Command::Lfc(false), "PSLFC OFF\r"),
            (Command::SetContainmentAmount(amount(4)), "PSCNTAMT 04\r"),
            (Command::ContainmentAmountUp, "PSCNTAMT UP\r"),
            (Command::ContainmentAmountDown, "PSCNTAMT DOWN\r"),
            (Command::SetDiracFilter(DiracFilter::Off), "PSDIRAC OFF\r"),
            (Command::SetDiracFilter(DiracFilter::Slot2), "PSDIRAC 2\r"),
        ];
        for (command, expected) in cases {
            for zone in Zone::ALL {
                assert_eq!(line(command, zone), expected, "{command:?} {zone:?}");
            }
        }
    }

    #[test]
    fn auto_lip_sync_depends_on_the_brand() {
        let mut out = String::new();
        for (brand, on, off) in [
            (Brand::Denon, "OPALSSET ON\r", "OPALSSET OFF\r"),
            (Brand::Marantz, "SSHOSALS ON\r", "SSHOSALS OFF\r"),
        ] {
            for (value, expected) in [(true, on), (false, off)] {
                out.clear();
                let command = Command::AutoLipSync(value);
                command.write_line(brand, Zone::Main, &mut out).unwrap();
                assert_eq!(out, expected, "{brand:?}");
            }
        }
    }

    #[test]
    fn remote_keys_depend_on_the_brand_and_the_zone() {
        let line = |brand, zone, command: Command<'_>| {
            let mut out = String::new();
            command.write_line(brand, zone, &mut out).map(|()| out)
        };
        let page_up = Command::RemoteKey(RemoteKey::PageUp);
        assert_eq!(
            line(Brand::Denon, Zone::Zone2, page_up).unwrap(),
            "RCKSK0430945\r"
        );
        assert_eq!(
            line(Brand::Marantz, Zone::Main, page_up).unwrap(),
            "RCRC52708105\r"
        );
        assert_eq!(
            line(Brand::Denon, Zone::Main, Command::DenonKeyCode(5)).unwrap(),
            "RCKSK0410005\r"
        );
        assert_eq!(
            line(Brand::Marantz, Zone::Main, Command::MarantzKeyCode("6012")).unwrap(),
            "RCRC56012\r"
        );
        assert_eq!(
            line(Brand::Marantz, Zone::Zone2, page_up),
            Err(Error::UnsupportedKey)
        );
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
            (Command::SetInputMode(InputMode::Auto), "SDAUTO\r"),
            (Command::SetInputMode(InputMode::Hdmi), "SDHDMI\r"),
            (Command::SetInputMode(InputMode::Digital), "SDDIGITAL\r"),
            (Command::SetInputMode(InputMode::Analog), "SDANALOG\r"),
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
            .write_line(Brand::Denon, Zone::Main, &mut out)
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
                let error = command
                    .write_line(Brand::Denon, Zone::Main, &mut out)
                    .unwrap_err();
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
