//! Commands as text, for telnet and for HTTP.

use std::fmt::Write as _;

use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{
    Brand, DeviceRanges, Setting, TunerCommand, VideoSettingValue, Zone, ZoneSettingValue,
};

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

/// As [`encode`], and refuses a value outside the ranges that the receiver
/// lists.
pub(crate) fn encode_checked(
    brand: Brand,
    ranges: &DeviceRanges,
    zone: Zone,
    command: &Command<'_>,
) -> Result<String, Error> {
    ranges.check(zone, command).map_err(Error::from_command)?;
    encode(brand, zone, command)
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

/// A set of [`Reads`], one bit for each.
pub(crate) type ReadBits = u16;

/// What a command changes that the receiver reports over HTTP.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reads {
    Power,
    Volume,
    Source,
    SoundMode,
    Device,
    /// Tone control, bass and treble of the main zone.
    Tone,
    /// The Audyssey settings.
    Audyssey,
    /// The band and the frequency of the tuner.
    Tuner,
    /// The HDMI output.
    HdmiOutput,
    /// The fast part of the settings of `AppCommand0300.xml`.
    Settings,
    /// The slow part of the settings of `AppCommand0300.xml`.
    SlowSettings,
}

impl Reads {
    /// The values that an HTTP read after the command in `zone` can show.
    /// `None` if no HTTP read has what the command changes.
    pub(crate) const fn after(zone: Zone, command: &Command<'_>) -> Option<Self> {
        match command {
            Command::PowerOn
            | Command::PowerStandby
            | Command::DevicePowerOn
            | Command::DevicePowerStandby => Some(Self::Power),
            Command::VolumeUp | Command::VolumeDown | Command::SetVolume(_) | Command::Mute(_) => {
                Some(Self::Volume)
            }
            // A quick select changes the source.
            Command::SelectSource(_)
            | Command::SetZoneSetting(ZoneSettingValue::QuickSelect(_)) => Some(Self::Source),
            Command::SelectSoundMode(_) => Some(Self::SoundMode),
            Command::SetDeviceSetting(_) => Some(Self::Device),
            // The reads of tone control and Audyssey have the main zone only.
            Command::ToneControl(_)
            | Command::SetBass(_)
            | Command::BassUp
            | Command::BassDown
            | Command::SetTreble(_)
            | Command::TrebleUp
            | Command::TrebleDown
                if matches!(zone, Zone::Main) =>
            {
                Some(Self::Tone)
            }
            Command::DynamicEq(_)
            | Command::SetMultiEq(_)
            | Command::SetReferenceLevelOffset(_)
            | Command::SetDynamicVolume(_)
                if matches!(zone, Zone::Main) =>
            {
                Some(Self::Audyssey)
            }
            // The tuner page has the band and frequency of the analog tuner
            // only. Tuning mode, storing a preset and HD Radio leave them as
            // they are.
            Command::Tuner(
                TunerCommand::FrequencyUp
                | TunerCommand::FrequencyDown
                | TunerCommand::Band(_)
                | TunerCommand::PresetUp
                | TunerCommand::PresetDown
                | TunerCommand::Preset(_),
            ) => Some(Self::Tuner),
            Command::SetVideoSetting(VideoSettingValue::HdmiOutput(_)) => Some(Self::HdmiOutput),
            _ => None,
        }
    }

    pub(crate) const fn bit(self) -> ReadBits {
        1 << self as ReadBits
    }

    /// The values that the receiver shows late: power, the source and the
    /// sound mode.
    pub(crate) const SLOW: ReadBits =
        Self::Power.bit() | Self::Source.bit() | Self::SoundMode.bit();

    /// The bit of the values that a change of this setting shows. 0 if the
    /// setting is not one of the late values.
    pub(crate) const fn of_setting(setting: Setting) -> ReadBits {
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
    use denon_marantz_proto::HdmiOutput;

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
        let main = |command| Reads::after(Zone::Main, &command);
        assert_eq!(main(Command::PowerOn), Some(Reads::Power));
        assert_eq!(main(Command::VolumeUp), Some(Reads::Volume));
        assert_eq!(main(Command::Mute(true)), Some(Reads::Volume));
        assert_eq!(main(Command::SelectSource("GAME")), Some(Reads::Source));
        assert_eq!(
            main(Command::SetZoneSetting(ZoneSettingValue::QuickSelect(1))),
            Some(Reads::Source)
        );
        assert_eq!(
            main(Command::SetZoneSetting(ZoneSettingValue::StoreQuickSelect(
                1
            ))),
            None
        );
        assert_eq!(
            main(Command::SelectSoundMode("MOVIE")),
            Some(Reads::SoundMode)
        );
        assert_eq!(main(Command::BassUp), Some(Reads::Tone));
        assert_eq!(main(Command::DynamicEq(true)), Some(Reads::Audyssey));
        assert_eq!(
            main(Command::Tuner(TunerCommand::FrequencyUp)),
            Some(Reads::Tuner)
        );
        assert_eq!(main(Command::Tuner(TunerCommand::StorePreset(1))), None);
        assert_eq!(main(Command::Tuner(TunerCommand::HdStorePreset(1))), None);
        assert_eq!(main(Command::Tuner(TunerCommand::HdFrequencyUp)), None);
        assert_eq!(main(Command::Tuner(TunerCommand::HdPreset(1))), None);
        assert_eq!(
            main(Command::Tuner(TunerCommand::HdMode(
                denon_marantz_proto::HdTuningMode::Manual
            ))),
            None
        );
        assert_eq!(main(Command::Play), None);
        assert_eq!(
            main(Command::SetVideoSetting(VideoSettingValue::HdmiOutput(
                HdmiOutput::Hdmi1
            ))),
            Some(Reads::HdmiOutput)
        );
        // The reads of tone control and Audyssey have the main zone only.
        assert_eq!(Reads::after(Zone::Zone2, &Command::BassUp), None);
        assert_eq!(Reads::after(Zone::Zone2, &Command::DynamicEq(true)), None);
        assert_eq!(
            Reads::after(Zone::Zone2, &Command::VolumeUp),
            Some(Reads::Volume)
        );
        let bits: Vec<ReadBits> = [
            Reads::Power,
            Reads::Volume,
            Reads::Source,
            Reads::SoundMode,
            Reads::Device,
            Reads::Tone,
            Reads::Audyssey,
            Reads::Tuner,
            Reads::HdmiOutput,
            Reads::Settings,
            Reads::SlowSettings,
        ]
        .iter()
        .map(|reads| reads.bit())
        .collect();
        assert_eq!(bits, [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024]);
    }
}
