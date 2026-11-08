//! Picture settings that the receiver reports on telnet as `PV` events. The
//! spec lists them for older receivers.

use core::fmt::Write as _;

use crate::error::Error;
use crate::values::{NoiseReduction, PictureMode};

/// Highest enhancer level.
const MAX_ENHANCER: u8 = 12;
/// Contrast, brightness and saturation go from -50 to +50.
const MAX_ADJUST: i8 = 50;

/// The picture settings of a receiver. A value is `None` until the receiver
/// reports it. A receiver only reports the settings that it has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PictureSettings {
    /// Picture mode.
    pub mode: Option<PictureMode>,
    /// Contrast, from -50 to +50.
    pub contrast: Option<i8>,
    /// Brightness, from -50 to +50.
    pub brightness: Option<i8>,
    /// Saturation, from -50 to +50.
    pub saturation: Option<i8>,
    /// Noise reduction.
    pub noise_reduction: Option<NoiseReduction>,
    /// Enhancer, from 0 to 12.
    pub enhancer: Option<u8>,
}

impl PictureSettings {
    /// Applies the text of a `PV` event, for example `MOV` or `CN 050`.
    /// Returns `None` if the text is not a picture setting that this library
    /// knows, and whether the value changed otherwise.
    pub(crate) fn apply(&mut self, parameter: &str) -> Option<bool> {
        let parameter = parameter.trim();
        let adjust = |value: &str| -> Option<i8> {
            let steps: i16 = value
                .trim()
                .parse()
                .ok()
                .filter(|steps| (0..=100).contains(steps))?;
            i8::try_from(steps - 50).ok()
        };
        if let Some(value) = parameter.strip_prefix("CN ") {
            put(&mut self.contrast, adjust(value))
        } else if let Some(value) = parameter.strip_prefix("BR ") {
            put(&mut self.brightness, adjust(value))
        } else if let Some(value) = parameter.strip_prefix("ST ") {
            put(&mut self.saturation, adjust(value))
        } else if let Some(value) = parameter.strip_prefix("DNR ") {
            put(
                &mut self.noise_reduction,
                NoiseReduction::from_code(value.trim()),
            )
        } else if let Some(value) = parameter.strip_prefix("ENH ") {
            let level = value
                .trim()
                .parse()
                .ok()
                .filter(|level| *level <= MAX_ENHANCER);
            put(&mut self.enhancer, level)
        } else {
            put(&mut self.mode, PictureMode::from_telnet(parameter))
        }
    }
}

fn put<T: PartialEq>(slot: &mut Option<T>, value: Option<T>) -> Option<bool> {
    let value = value?;
    let changed = slot.as_ref() != Some(&value);
    *slot = Some(value);
    Some(changed)
}

/// A picture setting with the value to set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PictureSettingValue {
    /// Sets the picture mode.
    Mode(PictureMode),
    /// Sets the contrast, from -50 to +50.
    Contrast(i8),
    /// Raises the contrast by one step.
    ContrastUp,
    /// Lowers the contrast by one step.
    ContrastDown,
    /// Sets the brightness, from -50 to +50.
    Brightness(i8),
    /// Raises the brightness by one step.
    BrightnessUp,
    /// Lowers the brightness by one step.
    BrightnessDown,
    /// Sets the saturation, from -50 to +50.
    Saturation(i8),
    /// Raises the saturation by one step.
    SaturationUp,
    /// Lowers the saturation by one step.
    SaturationDown,
    /// Sets the noise reduction.
    NoiseReduction(NoiseReduction),
    /// Sets the enhancer, from 0 to 12.
    Enhancer(u8),
    /// Raises the enhancer by one step.
    EnhancerUp,
    /// Lowers the enhancer by one step.
    EnhancerDown,
}

impl PictureSettingValue {
    /// Appends the command text, without the line end.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ValueOutOfRange`] for a value outside its range.
    /// `out` is not changed then.
    pub(crate) fn write(self, out: &mut String) -> Result<(), Error> {
        // Writing to a `String` cannot fail.
        let _ = match self {
            Self::Mode(mode) => write!(out, "PV{}", mode.telnet_code()),
            Self::Contrast(db) => return write_adjust(out, "CN", db, "contrast"),
            Self::ContrastUp => write!(out, "PVCN UP"),
            Self::ContrastDown => write!(out, "PVCN DOWN"),
            Self::Brightness(db) => return write_adjust(out, "BR", db, "brightness"),
            Self::BrightnessUp => write!(out, "PVBR UP"),
            Self::BrightnessDown => write!(out, "PVBR DOWN"),
            Self::Saturation(db) => return write_adjust(out, "ST", db, "saturation"),
            Self::SaturationUp => write!(out, "PVST UP"),
            Self::SaturationDown => write!(out, "PVST DOWN"),
            Self::NoiseReduction(level) => write!(out, "PVDNR {}", level.code()),
            Self::Enhancer(level) if level <= MAX_ENHANCER => write!(out, "PVENH {level:02}"),
            Self::Enhancer(_) => {
                return Err(Error::ValueOutOfRange { what: "enhancer" });
            }
            Self::EnhancerUp => write!(out, "PVENH UP"),
            Self::EnhancerDown => write!(out, "PVENH DOWN"),
        };
        Ok(())
    }
}

fn write_adjust(out: &mut String, name: &str, db: i8, what: &'static str) -> Result<(), Error> {
    if !(-MAX_ADJUST..=MAX_ADJUST).contains(&db) {
        return Err(Error::ValueOutOfRange { what });
    }
    let steps = i16::from(db) + 50;
    let _ = write!(out, "PV{name} {steps:03}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: PictureSettingValue) -> Result<String, Error> {
        let mut out = String::new();
        value.write(&mut out).map(|()| out)
    }

    #[test]
    fn texts() {
        let cases = [
            (PictureSettingValue::Mode(PictureMode::Movie), "PVMOV"),
            (PictureSettingValue::Mode(PictureMode::Standard), "PVSTD"),
            (PictureSettingValue::Mode(PictureMode::Custom), "PVCTM"),
            (PictureSettingValue::Mode(PictureMode::Off), "PVOFF"),
            (PictureSettingValue::Contrast(0), "PVCN 050"),
            (PictureSettingValue::Contrast(-50), "PVCN 000"),
            (PictureSettingValue::Brightness(50), "PVBR 100"),
            (PictureSettingValue::Saturation(-5), "PVST 045"),
            (PictureSettingValue::ContrastUp, "PVCN UP"),
            (PictureSettingValue::BrightnessDown, "PVBR DOWN"),
            (PictureSettingValue::SaturationUp, "PVST UP"),
            (
                PictureSettingValue::NoiseReduction(NoiseReduction::High),
                "PVDNR HI",
            ),
            (
                PictureSettingValue::NoiseReduction(NoiseReduction::Off),
                "PVDNR OFF",
            ),
            (PictureSettingValue::Enhancer(12), "PVENH 12"),
            (PictureSettingValue::Enhancer(0), "PVENH 00"),
            (PictureSettingValue::EnhancerUp, "PVENH UP"),
            (PictureSettingValue::EnhancerDown, "PVENH DOWN"),
        ];
        for (value, expected) in cases {
            assert_eq!(text(value).as_deref(), Ok(expected), "{value:?}");
        }
    }

    #[test]
    fn values_outside_the_range_are_refused() {
        for value in [
            PictureSettingValue::Contrast(51),
            PictureSettingValue::Brightness(-51),
            PictureSettingValue::Saturation(i8::MAX),
            PictureSettingValue::Enhancer(13),
        ] {
            assert!(
                matches!(text(value), Err(Error::ValueOutOfRange { .. })),
                "{value:?}"
            );
        }
    }

    #[test]
    fn events_fill_the_values() {
        let mut settings = PictureSettings::default();
        assert_eq!(settings.apply("MOV"), Some(true));
        assert_eq!(settings.apply("MOV"), Some(false));
        assert_eq!(settings.apply("CTM"), Some(true));
        assert_eq!(settings.apply("CN 050"), Some(true));
        assert_eq!(settings.apply("BR 070"), Some(true));
        assert_eq!(settings.apply("ST 000"), Some(true));
        assert_eq!(settings.apply("DNR LOW"), Some(true));
        assert_eq!(settings.apply("ENH 12"), Some(true));
        assert_eq!(settings.mode, Some(PictureMode::Custom));
        assert_eq!(settings.contrast, Some(0));
        assert_eq!(settings.brightness, Some(20));
        assert_eq!(settings.saturation, Some(-50));
        assert_eq!(settings.noise_reduction, Some(NoiseReduction::Low));
        assert_eq!(settings.enhancer, Some(12));
    }

    #[test]
    fn text_that_is_not_a_value_changes_nothing() {
        let mut settings = PictureSettings::default();
        for text in [
            "", "XYZ", "CN ", "CN 101", "CN x", "DNR XX", "ENH 13", "ST ?", "CN ?",
        ] {
            assert_eq!(settings.apply(text), None, "{text:?}");
        }
        assert_eq!(settings, PictureSettings::default());
    }
}
