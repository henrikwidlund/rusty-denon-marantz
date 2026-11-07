//! Zone settings that a client can set on telnet.

use core::fmt::Write as _;

use crate::error::Error;
use crate::tone::ToneLevel;
use crate::values::{MuteLevel, VolumeDisplay, ZoneChannelMode, ZoneHdmiAudio};
use crate::volume::{PowerOnVolume, Volume, VolumeMode};
use crate::zone::Zone;

/// Lowest and highest channel level in whole dB.
const MAX_CHANNEL_DB: i8 = 12;

/// A zone setting with the value to set.
///
/// Zone 2 and zone 3 have all settings except the volume display. The main
/// zone has the volume limit, the power on volume, the mute level and the
/// volume display only. Any other combination gives
/// [`Error::UnsupportedZone`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ZoneSettingValue {
    /// Bass level, from -10 to +10 dB.
    Bass(ToneLevel),
    /// Raises the bass by one step.
    BassUp,
    /// Lowers the bass by one step.
    BassDown,
    /// Treble level, from -10 to +10 dB.
    Treble(ToneLevel),
    /// Raises the treble by one step.
    TrebleUp,
    /// Lowers the treble by one step.
    TrebleDown,
    /// Level of the left channel in whole dB, from -12 to +12.
    LeftLevel(i8),
    /// Raises the level of the left channel by one step.
    LeftLevelUp,
    /// Lowers the level of the left channel by one step.
    LeftLevelDown,
    /// Level of the right channel in whole dB, from -12 to +12.
    RightLevel(i8),
    /// Raises the level of the right channel by one step.
    RightLevelUp,
    /// Lowers the level of the right channel by one step.
    RightLevelDown,
    /// High pass filter on (`true`) or off (`false`).
    HighPassFilter(bool),
    /// Stereo or mono.
    ChannelMode(ZoneChannelMode),
    /// Where the HDMI audio is decoded.
    HdmiAudio(ZoneHdmiAudio),
    /// Volume limit in whole dB, or none.
    VolumeLimit(Option<Volume>),
    /// Volume that the zone has when it is switched on. A level must be a
    /// whole dB.
    PowerOnVolume(PowerOnVolume),
    /// Mute level.
    MuteLevel(MuteLevel),
    /// Variable volume, or a fixed volume that must be a whole dB.
    VolumeMode(VolumeMode),
    /// How the volume is shown, relative to 0 dB or as an absolute number.
    VolumeDisplay(VolumeDisplay),
}

impl ZoneSettingValue {
    /// Appends the command text for `zone`, without the line end.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedZone`] for the main zone, and
    /// [`Error::ValueOutOfRange`] for a level that the receiver does not take.
    /// `out` is not changed then.
    pub(crate) fn write(self, zone: Zone, out: &mut String) -> Result<(), Error> {
        let start = out.len();
        let result = self.write_text(zone, out);
        if result.is_err() {
            out.truncate(start);
        }
        result
    }

    fn write_text(self, zone: Zone, out: &mut String) -> Result<(), Error> {
        let z = match zone {
            Zone::Main => return self.write_volume_setting(zone, out),
            Zone::Zone2 => "Z2",
            Zone::Zone3 => "Z3",
        };
        match self {
            Self::Bass(level) => push_level(out, z, "PSBAS", level),
            Self::BassUp => push(out, z, "PSBAS UP"),
            Self::BassDown => push(out, z, "PSBAS DOWN"),
            Self::Treble(level) => push_level(out, z, "PSTRE", level),
            Self::TrebleUp => push(out, z, "PSTRE UP"),
            Self::TrebleDown => push(out, z, "PSTRE DOWN"),
            Self::LeftLevel(db) => push_channel(out, z, "CVFL", db)?,
            Self::LeftLevelUp => push(out, z, "CVFL UP"),
            Self::LeftLevelDown => push(out, z, "CVFL DOWN"),
            Self::RightLevel(db) => push_channel(out, z, "CVFR", db)?,
            Self::RightLevelUp => push(out, z, "CVFR UP"),
            Self::RightLevelDown => push(out, z, "CVFR DOWN"),
            Self::HighPassFilter(on) => push(out, z, if on { "HPFON" } else { "HPFOFF" }),
            Self::ChannelMode(mode) => {
                let _ = write!(out, "{z}CS{}", mode.code());
            }
            Self::HdmiAudio(mode) => {
                let _ = write!(out, "{z}HDA {}", mode.code());
            }
            Self::VolumeLimit(_)
            | Self::PowerOnVolume(_)
            | Self::MuteLevel(_)
            | Self::VolumeMode(_)
            | Self::VolumeDisplay(_) => return self.write_volume_setting(zone, out),
        }
        Ok(())
    }

    /// Writes a volume setting. The main zone writes the limit with two digits
    /// and the other zones with three. The volume mode is for zone 2 and zone
    /// 3, and the volume display for the main zone.
    fn write_volume_setting(self, zone: Zone, out: &mut String) -> Result<(), Error> {
        let main = matches!(zone, Zone::Main);
        let setup = match zone {
            Zone::Main => "SSVCTZMA",
            Zone::Zone2 => "SSVCTZ2S",
            Zone::Zone3 => "SSVCTZ3S",
        };
        let limit_width = if main { 2 } else { 3 };
        match self {
            Self::VolumeLimit(None) => push_setup(out, setup, "LIM", "OFF"),
            Self::VolumeLimit(Some(limit)) => {
                let db = whole_db(limit, "volume limit")?;
                let _ = write!(out, "{setup}LIM {db:0limit_width$}");
            }
            Self::PowerOnVolume(PowerOnVolume::Muted) => push_setup(out, setup, "PON", "MUT"),
            Self::PowerOnVolume(PowerOnVolume::Last) => push_setup(out, setup, "PON", "LAS"),
            Self::PowerOnVolume(PowerOnVolume::Level(level)) => {
                let _ = write!(out, "{setup}PON {:02}", whole_db(level, "power on volume")?);
            }
            Self::MuteLevel(level) => push_setup(out, setup, "MLV", level.code()),
            Self::VolumeMode(VolumeMode::Variable) if !main => {
                push_setup(out, setup, "LEV", "VAR");
            }
            Self::VolumeMode(VolumeMode::Fixed(level)) if !main => {
                let _ = write!(out, "{setup}LEV {:02}", whole_db(level, "fixed volume")?);
            }
            Self::VolumeDisplay(display) if main => {
                push_setup(out, setup, "DIS", display.code());
            }
            _ => return Err(Error::UnsupportedZone),
        }
        Ok(())
    }
}

fn push(out: &mut String, z: &str, text: &str) {
    out.push_str(z);
    out.push_str(text);
}

fn push_level(out: &mut String, z: &str, name: &str, level: ToneLevel) {
    // Writing to a `String` cannot fail.
    let _ = write!(out, "{z}{name} {level}");
}

fn push_channel(out: &mut String, z: &str, name: &str, db: i8) -> Result<(), Error> {
    if !(-MAX_CHANNEL_DB..=MAX_CHANNEL_DB).contains(&db) {
        return Err(Error::ValueOutOfRange {
            what: "channel level",
        });
    }
    let code = 50_i16 + i16::from(db);
    let _ = write!(out, "{z}{name} {code:02}");
    Ok(())
}

fn push_setup(out: &mut String, setup: &str, kind: &str, value: &str) {
    let _ = write!(out, "{setup}{kind} {value}");
}

/// The volume in whole dB above -80 dB. The receiver takes no half steps.
const fn whole_db(volume: Volume, what: &'static str) -> Result<u8, Error> {
    let half = volume.half_db_above_min();
    if half & 1 == 1 {
        Err(Error::ValueOutOfRange { what })
    } else {
        Ok(half >> 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: ZoneSettingValue, zone: Zone) -> Result<String, Error> {
        let mut out = String::new();
        value.write(zone, &mut out).map(|()| out)
    }

    fn volume(db: f32) -> Volume {
        Volume::from_db_rounded(db).unwrap()
    }

    #[test]
    fn texts_of_zone_2() {
        let tone = |db| ToneLevel::from_db(db).unwrap();
        let cases = [
            (ZoneSettingValue::Bass(tone(8)), "Z2PSBAS 58"),
            (ZoneSettingValue::Bass(tone(-10)), "Z2PSBAS 40"),
            (ZoneSettingValue::BassUp, "Z2PSBAS UP"),
            (ZoneSettingValue::BassDown, "Z2PSBAS DOWN"),
            (ZoneSettingValue::Treble(tone(0)), "Z2PSTRE 50"),
            (ZoneSettingValue::TrebleUp, "Z2PSTRE UP"),
            (ZoneSettingValue::TrebleDown, "Z2PSTRE DOWN"),
            (ZoneSettingValue::LeftLevel(2), "Z2CVFL 52"),
            (ZoneSettingValue::LeftLevel(-12), "Z2CVFL 38"),
            (ZoneSettingValue::LeftLevelUp, "Z2CVFL UP"),
            (ZoneSettingValue::LeftLevelDown, "Z2CVFL DOWN"),
            (ZoneSettingValue::RightLevel(-2), "Z2CVFR 48"),
            (ZoneSettingValue::RightLevelUp, "Z2CVFR UP"),
            (ZoneSettingValue::RightLevelDown, "Z2CVFR DOWN"),
            (ZoneSettingValue::HighPassFilter(true), "Z2HPFON"),
            (ZoneSettingValue::HighPassFilter(false), "Z2HPFOFF"),
            (
                ZoneSettingValue::ChannelMode(ZoneChannelMode::Mono),
                "Z2CSMONO",
            ),
            (
                ZoneSettingValue::ChannelMode(ZoneChannelMode::Stereo),
                "Z2CSST",
            ),
            (ZoneSettingValue::HdmiAudio(ZoneHdmiAudio::Pcm), "Z2HDA PCM"),
            (
                ZoneSettingValue::HdmiAudio(ZoneHdmiAudio::Through),
                "Z2HDA THR",
            ),
        ];
        for (value, expected) in cases {
            assert_eq!(
                text(value, Zone::Zone2).as_deref(),
                Ok(expected),
                "{value:?}"
            );
        }
    }

    #[test]
    fn volume_settings_of_zone_2() {
        let cases = [
            (ZoneSettingValue::VolumeLimit(None), "SSVCTZ2SLIM OFF"),
            (
                ZoneSettingValue::VolumeLimit(Some(volume(-10.0))),
                "SSVCTZ2SLIM 070",
            ),
            (
                ZoneSettingValue::VolumeLimit(Some(volume(0.0))),
                "SSVCTZ2SLIM 080",
            ),
            (
                ZoneSettingValue::PowerOnVolume(PowerOnVolume::Muted),
                "SSVCTZ2SPON MUT",
            ),
            (
                ZoneSettingValue::PowerOnVolume(PowerOnVolume::Last),
                "SSVCTZ2SPON LAS",
            ),
            (
                ZoneSettingValue::PowerOnVolume(PowerOnVolume::Level(volume(-40.0))),
                "SSVCTZ2SPON 40",
            ),
            (
                ZoneSettingValue::MuteLevel(MuteLevel::Full),
                "SSVCTZ2SMLV MUT",
            ),
            (
                ZoneSettingValue::MuteLevel(MuteLevel::Minus20Db),
                "SSVCTZ2SMLV 060",
            ),
            (
                ZoneSettingValue::VolumeMode(VolumeMode::Variable),
                "SSVCTZ2SLEV VAR",
            ),
            (
                ZoneSettingValue::VolumeMode(VolumeMode::Fixed(volume(-70.0))),
                "SSVCTZ2SLEV 10",
            ),
        ];
        for (value, expected) in cases {
            assert_eq!(
                text(value, Zone::Zone2).as_deref(),
                Ok(expected),
                "{value:?}"
            );
        }
    }

    #[test]
    fn zone_3_uses_its_own_prefix() {
        assert_eq!(
            text(ZoneSettingValue::BassUp, Zone::Zone3).unwrap(),
            "Z3PSBAS UP"
        );
        assert_eq!(
            text(ZoneSettingValue::HighPassFilter(true), Zone::Zone3).unwrap(),
            "Z3HPFON"
        );
        assert_eq!(
            text(ZoneSettingValue::VolumeLimit(None), Zone::Zone3).unwrap(),
            "SSVCTZ3SLIM OFF"
        );
    }

    #[test]
    fn the_main_zone_has_the_volume_settings_only() {
        let cases = [
            (ZoneSettingValue::VolumeLimit(None), "SSVCTZMALIM OFF"),
            (
                ZoneSettingValue::VolumeLimit(Some(volume(-20.0))),
                "SSVCTZMALIM 60",
            ),
            (
                ZoneSettingValue::PowerOnVolume(PowerOnVolume::Level(volume(-45.0))),
                "SSVCTZMAPON 35",
            ),
            (
                ZoneSettingValue::PowerOnVolume(PowerOnVolume::Last),
                "SSVCTZMAPON LAS",
            ),
            (
                ZoneSettingValue::MuteLevel(MuteLevel::Minus40Db),
                "SSVCTZMAMLV 040",
            ),
            (
                ZoneSettingValue::VolumeDisplay(VolumeDisplay::Absolute),
                "SSVCTZMADIS ABS",
            ),
        ];
        for (value, expected) in cases {
            assert_eq!(
                text(value, Zone::Main).as_deref(),
                Ok(expected),
                "{value:?}"
            );
        }
        for value in [
            ZoneSettingValue::BassUp,
            ZoneSettingValue::HighPassFilter(true),
            ZoneSettingValue::VolumeMode(VolumeMode::Variable),
        ] {
            assert_eq!(
                text(value, Zone::Main),
                Err(Error::UnsupportedZone),
                "{value:?}"
            );
        }
    }

    #[test]
    fn the_volume_display_is_for_the_main_zone_only() {
        let display = ZoneSettingValue::VolumeDisplay(VolumeDisplay::Relative);
        assert_eq!(text(display, Zone::Zone2), Err(Error::UnsupportedZone));
        assert_eq!(text(display, Zone::Zone3), Err(Error::UnsupportedZone));
    }

    #[test]
    fn values_the_receiver_does_not_take() {
        for value in [
            ZoneSettingValue::LeftLevel(13),
            ZoneSettingValue::RightLevel(-13),
            ZoneSettingValue::VolumeLimit(Some(volume(-9.5))),
            ZoneSettingValue::PowerOnVolume(PowerOnVolume::Level(volume(-39.5))),
            ZoneSettingValue::VolumeMode(VolumeMode::Fixed(volume(-69.5))),
        ] {
            assert!(
                matches!(text(value, Zone::Zone2), Err(Error::ValueOutOfRange { .. })),
                "{value:?}"
            );
        }
        let mut out = String::from("x");
        assert!(
            ZoneSettingValue::LeftLevel(13)
                .write(Zone::Zone2, &mut out)
                .is_err()
        );
        assert_eq!(out, "x");
    }
}
