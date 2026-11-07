//! Commands and state of the tuner.

use core::fmt;
use core::fmt::Write as _;

use crate::error::Error;
use crate::values::{TunerBand, TunerMode};

/// Highest preset number of the tuner.
const MAX_PRESET: u8 = 56;

/// Frequency of the tuner as the receiver reports it: six digits written as
/// `****.**`. Values above `050000` are AM in kHz, the others FM in MHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TunerFrequency {
    hundredths: u32,
}

impl TunerFrequency {
    /// Reads the six digits of the report, for example `105000` or `008750`.
    #[must_use]
    pub fn from_digits(digits: &str) -> Option<Self> {
        if digits.len() != 6 || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        digits.parse().ok().map(|hundredths| Self { hundredths })
    }

    /// The band that the frequency belongs to: AM or FM.
    #[must_use]
    pub const fn band(self) -> TunerBand {
        if self.hundredths > 50_000 {
            TunerBand::Am
        } else {
            TunerBand::Fm
        }
    }

    /// The frequency in hundredths: of kHz for AM, of MHz for FM.
    #[must_use]
    pub const fn hundredths(self) -> u32 {
        self.hundredths
    }
}

impl fmt::Display for TunerFrequency {
    /// For example `87.50 MHz` or `1050.00 kHz`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let unit = match self.band() {
            TunerBand::Am => "kHz",
            _ => "MHz",
        };
        write!(
            f,
            "{}.{:02} {unit}",
            self.hundredths / 100,
            self.hundredths % 100
        )
    }
}

/// What the tuner reports. A value is `None` until the receiver reports it,
/// which it does for the tuner source only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TunerState {
    /// Band. The frequency also gives AM or FM.
    pub band: Option<TunerBand>,
    /// Tuning mode.
    pub mode: Option<TunerMode>,
    /// Frequency.
    pub frequency: Option<TunerFrequency>,
    /// Preset number from 1 to 56. `None` if the tuner is not on a preset.
    pub preset: Option<u8>,
    /// Name of the station from RDS, in Europe and Asia Pacific models.
    pub station_name: Option<String>,
}

impl TunerState {
    /// Applies the text of a `TF` event, for example `AN105000` or
    /// `ANNAME BBC R2 `. Returns `None` if it is not a value that this library
    /// knows, and whether the state changed otherwise.
    pub(crate) fn apply_frequency(&mut self, parameter: &str) -> Option<bool> {
        let rest = parameter.strip_prefix("AN")?;
        if let Some(name) = rest.strip_prefix("NAME") {
            // The receiver writes a space as an underscore.
            let name = name.replace('_', " ");
            let name = name.trim();
            let name = (!name.is_empty()).then(|| name.to_owned());
            return Some(replace(&mut self.station_name, name));
        }
        let frequency = TunerFrequency::from_digits(rest)?;
        let band = replace(&mut self.band, Some(frequency.band()));
        Some(replace(&mut self.frequency, Some(frequency)) | band)
    }

    /// Applies the text of a `TP` event, for example `AN01` or `ANOFF`.
    pub(crate) fn apply_preset(&mut self, parameter: &str) -> Option<bool> {
        let rest = parameter.strip_prefix("AN")?;
        let preset = if rest == "OFF" {
            None
        } else {
            Some(parse_preset(rest)?)
        };
        Some(replace(&mut self.preset, preset))
    }

    /// Applies the text of a `TM` event, for example `ANFM` or `ANAUTO`.
    pub(crate) fn apply_band_or_mode(&mut self, parameter: &str) -> Option<bool> {
        let code = parameter.strip_prefix("AN")?;
        if let Some(band) = TunerBand::from_code(code) {
            Some(replace(&mut self.band, Some(band)))
        } else {
            let mode = TunerMode::from_code(code)?;
            Some(replace(&mut self.mode, Some(mode)))
        }
    }
}

/// Reads a preset number: two digits from 01 to 56.
fn parse_preset(text: &str) -> Option<u8> {
    if text.len() != 2 || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse()
        .ok()
        .filter(|number| (1..=MAX_PRESET).contains(number))
}

/// Replaces the value. Returns true if it was different.
fn replace<T: PartialEq>(slot: &mut Option<T>, value: Option<T>) -> bool {
    if *slot == value {
        false
    } else {
        *slot = value;
        true
    }
}

/// A command for the tuner. Only receivers with a tuner accept them, and the
/// source must be the tuner. Some models have a tuner in some regions only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TunerCommand {
    /// Tunes to the next frequency.
    FrequencyUp,
    /// Tunes to the previous frequency.
    FrequencyDown,
    /// Selects the band. DAB is for DAB models only.
    Band(TunerBand),
    /// Selects automatic or manual tuning.
    Mode(TunerMode),
    /// Goes to the next preset.
    PresetUp,
    /// Goes to the previous preset.
    PresetDown,
    /// Goes to a preset, from 1 to 56.
    Preset(u8),
    /// Stores the station in a preset, from 1 to 56.
    StorePreset(u8),
}

impl TunerCommand {
    /// Appends the command text, without the line end.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ValueOutOfRange`] for a preset that is not from 1 to
    /// 56. `out` is not changed then.
    pub(crate) fn write(self, out: &mut String) -> Result<(), Error> {
        match self {
            Self::FrequencyUp => out.push_str("TFANUP"),
            Self::FrequencyDown => out.push_str("TFANDOWN"),
            Self::Band(band) => {
                out.push_str("TMAN");
                out.push_str(band.code());
            }
            Self::Mode(mode) => {
                out.push_str("TMAN");
                out.push_str(mode.code());
            }
            Self::PresetUp => out.push_str("TPANUP"),
            Self::PresetDown => out.push_str("TPANDOWN"),
            Self::Preset(number) => write_preset(out, "TPAN", number)?,
            Self::StorePreset(number) => write_preset(out, "TPANMEM", number)?,
        }
        Ok(())
    }
}

fn write_preset(out: &mut String, start: &str, number: u8) -> Result<(), Error> {
    if !(1..=MAX_PRESET).contains(&number) {
        return Err(Error::ValueOutOfRange {
            what: "tuner preset",
        });
    }
    out.push_str(start);
    // Writing to a `String` cannot fail.
    let _ = write!(out, "{number:02}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(command: TunerCommand) -> Result<String, Error> {
        let mut out = String::new();
        command.write(&mut out).map(|()| out)
    }

    #[test]
    fn texts() {
        let cases = [
            (TunerCommand::FrequencyUp, "TFANUP"),
            (TunerCommand::FrequencyDown, "TFANDOWN"),
            (TunerCommand::Band(TunerBand::Fm), "TMANFM"),
            (TunerCommand::Band(TunerBand::Am), "TMANAM"),
            (TunerCommand::Band(TunerBand::Dab), "TMANDAB"),
            (TunerCommand::Mode(TunerMode::Auto), "TMANAUTO"),
            (TunerCommand::Mode(TunerMode::Manual), "TMANMANUAL"),
            (TunerCommand::PresetUp, "TPANUP"),
            (TunerCommand::PresetDown, "TPANDOWN"),
            (TunerCommand::Preset(1), "TPAN01"),
            (TunerCommand::Preset(56), "TPAN56"),
            (TunerCommand::StorePreset(7), "TPANMEM07"),
        ];
        for (command, expected) in cases {
            assert_eq!(text(command).as_deref(), Ok(expected), "{command:?}");
        }
    }

    #[test]
    fn presets_are_from_1_to_56() {
        for number in [0, 57, 255] {
            for command in [
                TunerCommand::Preset(number),
                TunerCommand::StorePreset(number),
            ] {
                assert_eq!(
                    text(command),
                    Err(Error::ValueOutOfRange {
                        what: "tuner preset"
                    })
                );
            }
        }
    }

    #[test]
    fn frequency_digits() {
        let fm = TunerFrequency::from_digits("008750").unwrap();
        assert_eq!(fm.band(), TunerBand::Fm);
        assert_eq!(fm.to_string(), "87.50 MHz");
        let am = TunerFrequency::from_digits("105000").unwrap();
        assert_eq!(am.band(), TunerBand::Am);
        assert_eq!(am.to_string(), "1050.00 kHz");
        assert_eq!(am.hundredths(), 105_000);
        for digits in ["", "10500", "1050000", "10500x", "-10500"] {
            assert_eq!(TunerFrequency::from_digits(digits), None, "{digits:?}");
        }
    }

    #[test]
    fn events_fill_the_state() {
        let mut state = TunerState::default();
        assert_eq!(state.apply_frequency("AN0105"), None);
        assert_eq!(state.apply_frequency("AN010500"), Some(true));
        assert_eq!(state.apply_frequency("AN010500"), Some(false));
        assert_eq!(state.band, Some(TunerBand::Fm));
        assert_eq!(state.apply_frequency("AN105000"), Some(true));
        assert_eq!(state.band, Some(TunerBand::Am));
        assert_eq!(state.apply_frequency("ANNAMEBBC_R2__"), Some(true));
        assert_eq!(state.station_name.as_deref(), Some("BBC R2"));
        assert_eq!(state.apply_frequency("ANNAME________"), Some(true));
        assert_eq!(state.station_name, None);
        assert_eq!(state.apply_preset("AN07"), Some(true));
        assert_eq!(state.preset, Some(7));
        assert_eq!(state.apply_preset("ANOFF"), Some(true));
        assert_eq!(state.preset, None);
        assert_eq!(state.apply_band_or_mode("ANFM"), Some(true));
        assert_eq!(state.apply_band_or_mode("ANMANUAL"), Some(true));
        assert_eq!(state.band, Some(TunerBand::Fm));
        assert_eq!(state.mode, Some(TunerMode::Manual));
    }

    #[test]
    fn events_that_are_not_values_change_nothing() {
        let mut state = TunerState::default();
        for text in ["", "UP", "AN", "ANUP", "AN?", "ANxyz"] {
            assert_eq!(state.apply_frequency(text), None, "{text:?}");
            assert_eq!(state.apply_band_or_mode(text), None, "{text:?}");
        }
        for text in ["", "ANUP", "AN00", "AN57", "ANMEM", "AN1"] {
            assert_eq!(state.apply_preset(text), None, "{text:?}");
        }
        assert_eq!(state, TunerState::default());
    }

    #[test]
    fn a_failed_command_leaves_the_text_unchanged() {
        let mut out = String::from("x");
        assert!(TunerCommand::Preset(0).write(&mut out).is_err());
        assert_eq!(out, "x");
    }
}
