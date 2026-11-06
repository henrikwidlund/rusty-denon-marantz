//! Sound settings that the receiver reports on telnet as `PS` events.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::error::Error;
use crate::{
    AudioRestorer, Auro3dMode, AuroMatic3dPreset, ChannelLevel, DacFilter, DialogEnhancerLevel,
    Drc, EffectSpeakerSelection, ImaxAudio, ImaxMode, ImaxSubwooferOutput, Mdax, RoomSize,
    SurroundLevelCompensation,
};

/// Highest value of each number, as the receiver accepts it.
const MAX_AURO_STRENGTH: u16 = 15;
const MAX_BASS_SYNC: u16 = 16;
const MAX_LFE_LEVEL: u16 = 10;
const MAX_DIALOG_CONTROL: u16 = 6;
const MAX_DELAY_TIME: u16 = 300;
const MAX_AUDIO_DELAY: u16 = 500;
const MAX_DIMENSION: u16 = 6;
const MAX_CENTER_WIDTH: u16 = 7;
/// Values of the IMAX filters in Hz.
const IMAX_HIGH_PASS_HZ: [u16; 11] = [40, 60, 80, 90, 100, 110, 120, 150, 180, 200, 250];
const IMAX_LOW_PASS_HZ: [u16; 9] = [80, 90, 100, 110, 120, 150, 180, 200, 250];

/// Most `other` entries that are kept. A faulty receiver cannot make the
/// table grow past it.
const MAX_OTHER: usize = 256;

/// The sound settings of a receiver. A value is `None` until the receiver
/// reports it. A setting that has a name or a value that this library does not
/// know is kept as text in [`other`](Self::other).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoundSettings {
    /// Audio restorer.
    pub audio_restorer: Option<AudioRestorer>,
    /// Room size.
    pub room_size: Option<RoomSize>,
    /// Surround level compensation.
    pub surround_level: Option<SurroundLevelCompensation>,
    /// Dimension, from 0 to 6.
    pub dimension: Option<u8>,
    /// Center width, from 0 to 7.
    pub center_width: Option<u8>,
    /// Dialog enhancer.
    pub dialog_enhancer: Option<DialogEnhancerLevel>,
    /// Dynamic range compression.
    pub drc: Option<Drc>,
    /// Auro-3D mode.
    pub auro_3d_mode: Option<Auro3dMode>,
    /// Auro-Matic preset.
    pub auro_matic_preset: Option<AuroMatic3dPreset>,
    /// Auro-Matic strength, from 0 to 15.
    pub auro_matic_strength: Option<u8>,
    /// Speakers used for the effect channels.
    pub effect_speakers: Option<EffectSpeakerSelection>,
    /// MDAX level. Marantz receivers only.
    pub mdax: Option<Mdax>,
    /// DAC filter. Marantz receivers only.
    pub dac_filter: Option<DacFilter>,
    /// IMAX mode.
    pub imax: Option<ImaxMode>,
    /// IMAX audio settings.
    pub imax_audio: Option<ImaxAudio>,
    /// IMAX subwoofer output.
    pub imax_subwoofer_output: Option<ImaxSubwooferOutput>,
    /// IMAX subwoofer mode is on.
    pub imax_subwoofer_mode: Option<bool>,
    /// IMAX high pass filter in Hz.
    pub imax_high_pass: Option<u16>,
    /// IMAX low pass filter in Hz.
    pub imax_low_pass: Option<u16>,
    /// The subwoofer is on.
    pub subwoofer: Option<bool>,
    /// Level of subwoofer 1 to 4. The first is subwoofer 1.
    pub subwoofer_levels: [Option<ChannelLevel>; 4],
    /// LFE level, as dB below 0 dB, from 0 to 10.
    pub lfe_level: Option<u8>,
    /// Level of the center speaker.
    pub center_level: Option<ChannelLevel>,
    /// Neural:X is on.
    pub neural_x: Option<bool>,
    /// Panorama is on.
    pub panorama: Option<bool>,
    /// Center spread is on.
    pub center_spread: Option<bool>,
    /// Loudness management is on.
    pub loudness_management: Option<bool>,
    /// Speaker virtualizer is on.
    pub speaker_virtualizer: Option<bool>,
    /// Graphic EQ is on.
    pub graphic_eq: Option<bool>,
    /// Headphone EQ is on.
    pub headphone_eq: Option<bool>,
    /// Cinema EQ is on.
    pub cinema_eq: Option<bool>,
    /// Bass sync in steps, from 0 to 16.
    pub bass_sync: Option<u8>,
    /// Dialog control in dB, from 0 to 6.
    pub dialog_control: Option<u8>,
    /// Delay time in ms, from 0 to 300.
    pub delay_time: Option<u16>,
    /// Audio delay in ms, from 0 to 500.
    pub audio_delay: Option<u16>,
    /// Settings that have a name or a value that this library does not know.
    /// The key is the name without `PS`, the value is the text that follows
    /// it.
    pub other: BTreeMap<String, String>,
}

impl SoundSettings {
    /// Applies a `PS` event. `name` is the name without `PS`, for example
    /// `RSTR`, and `value` is the text that follows it. Returns true if
    /// something changed. Events that the state keeps in other values, such
    /// as the tone control, do not come here.
    pub(crate) fn apply(&mut self, name: &str, value: &str) -> bool {
        let value = value.trim();
        self.apply_known(name, value)
            .unwrap_or_else(|| self.apply_other(name, value))
    }

    /// `None` if the name or the value is not known.
    fn apply_known(&mut self, name: &str, value: &str) -> Option<bool> {
        match name {
            "RSTR" => put(&mut self.audio_restorer, AudioRestorer::from_code(value)),
            "RSZ" => put(&mut self.room_size, RoomSize::from_code(value)),
            "SURLEV" => put(
                &mut self.surround_level,
                SurroundLevelCompensation::from_code(value),
            ),
            "DIM" => put(&mut self.dimension, number(value, MAX_DIMENSION)),
            // The protocol spec writes `CEN`, another source writes `CEW`.
            "CEN" | "CEW" => put(&mut self.center_width, number(value, MAX_CENTER_WIDTH)),
            "DEH" => put(
                &mut self.dialog_enhancer,
                DialogEnhancerLevel::from_code(value),
            ),
            "DRC" => put(&mut self.drc, Drc::from_code(value)),
            "AUROMODE" => put(&mut self.auro_3d_mode, Auro3dMode::from_code(value)),
            "AUROPR" => put(
                &mut self.auro_matic_preset,
                AuroMatic3dPreset::from_code(value),
            ),
            "AUROST" => put(
                &mut self.auro_matic_strength,
                number(value, MAX_AURO_STRENGTH),
            ),
            "SP:" => put(
                &mut self.effect_speakers,
                EffectSpeakerSelection::from_code(value),
            ),
            "MDAX" => put(&mut self.mdax, Mdax::from_code(value)),
            "DACFIL" => put(&mut self.dac_filter, DacFilter::from_code(value)),
            "IMAX" => put(&mut self.imax, ImaxMode::from_code(value)),
            "IMAXAUD" => put(&mut self.imax_audio, ImaxAudio::from_code(value)),
            "IMAXSWO" => put(
                &mut self.imax_subwoofer_output,
                ImaxSubwooferOutput::from_code(value),
            ),
            "IMAXSWM" => put(&mut self.imax_subwoofer_mode, on_off(value)),
            "IMAXHPF" => put(&mut self.imax_high_pass, number(value, 999)),
            "IMAXLPF" => put(&mut self.imax_low_pass, number(value, 999)),
            "SWR" => put(&mut self.subwoofer, on_off(value)),
            "SWL" => put(
                &mut self.subwoofer_levels[0],
                ChannelLevel::from_code(value),
            ),
            "SWL2" => put(
                &mut self.subwoofer_levels[1],
                ChannelLevel::from_code(value),
            ),
            "SWL3" => put(
                &mut self.subwoofer_levels[2],
                ChannelLevel::from_code(value),
            ),
            "SWL4" => put(
                &mut self.subwoofer_levels[3],
                ChannelLevel::from_code(value),
            ),
            "LFE" => put(&mut self.lfe_level, number(value, MAX_LFE_LEVEL)),
            "CLV" => put(&mut self.center_level, ChannelLevel::from_code(value)),
            "NEURAL" => put(&mut self.neural_x, on_off(value)),
            "PAN" => put(&mut self.panorama, on_off(value)),
            "CES" => put(&mut self.center_spread, on_off(value)),
            "LOM" => put(&mut self.loudness_management, on_off(value)),
            "SPV" => put(&mut self.speaker_virtualizer, on_off(value)),
            "GEQ" => put(&mut self.graphic_eq, on_off(value)),
            "HEQ" => put(&mut self.headphone_eq, on_off(value)),
            // The event is `PSCINEMA EQ.ON`, so the name ends at the space.
            "CINEMA" => put(&mut self.cinema_eq, on_off(value.strip_prefix("EQ.")?)),
            "BSC" => put(&mut self.bass_sync, number(value, MAX_BASS_SYNC)),
            "DIC" => put(&mut self.dialog_control, number(value, MAX_DIALOG_CONTROL)),
            "DEL" => put(&mut self.delay_time, number(value, MAX_DELAY_TIME)),
            "DELAY" => put(&mut self.audio_delay, number(value, MAX_AUDIO_DELAY)),
            _ => None,
        }
    }

    /// Keeps the text of a setting that is not known. Names that are not
    /// short letters and digits, and empty values, are ignored.
    fn apply_other(&mut self, name: &str, value: &str) -> bool {
        let valid = (1..=16).contains(&name.len())
            && name.bytes().all(|b| b.is_ascii_alphanumeric())
            && !value.is_empty()
            && value != "?";
        if !valid {
            return false;
        }
        let is_full = self.other.len() >= MAX_OTHER;
        match self.other.get_mut(name) {
            Some(current) if current == value => false,
            Some(current) => {
                current.clear();
                current.push_str(value);
                true
            }
            None if is_full => false,
            None => {
                self.other.insert(name.to_owned(), value.to_owned());
                true
            }
        }
    }
}

/// A sound setting to send to the receiver. The text depends on the setting.
/// A number outside the range that the receiver accepts is an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SoundSettingValue {
    /// Audio restorer.
    AudioRestorer(AudioRestorer),
    /// Room size.
    RoomSize(RoomSize),
    /// Surround level compensation.
    SurroundLevel(SurroundLevelCompensation),
    /// Dialog enhancer.
    DialogEnhancer(DialogEnhancerLevel),
    /// Dynamic range compression.
    Drc(Drc),
    /// Auro-3D mode.
    Auro3dMode(Auro3dMode),
    /// Auro-Matic preset.
    AuroMaticPreset(AuroMatic3dPreset),
    /// Auro-Matic strength, from 0 to 15.
    AuroMaticStrength(u8),
    /// Speakers used for the effect channels.
    EffectSpeakers(EffectSpeakerSelection),
    /// MDAX level. Marantz receivers only.
    Mdax(Mdax),
    /// DAC filter. Marantz receivers only.
    DacFilter(DacFilter),
    /// IMAX mode.
    Imax(ImaxMode),
    /// IMAX audio settings.
    ImaxAudio(ImaxAudio),
    /// IMAX subwoofer output.
    ImaxSubwooferOutput(ImaxSubwooferOutput),
    /// IMAX subwoofer mode.
    ImaxSubwooferMode(bool),
    /// IMAX high pass filter in Hz: 40, 60, 80, 90, 100, 110, 120, 150, 180,
    /// 200 or 250.
    ImaxHighPass(u16),
    /// IMAX low pass filter in Hz: 80, 90, 100, 110, 120, 150, 180, 200 or
    /// 250.
    ImaxLowPass(u16),
    /// Subwoofer on or off.
    Subwoofer(bool),
    /// Level of a subwoofer. `number` is 1 to 4.
    SubwooferLevel {
        /// Subwoofer number, from 1 to 4.
        number: u8,
        /// The level.
        level: ChannelLevel,
    },
    /// LFE level, as dB below 0 dB, from 0 to 10.
    LfeLevel(u8),
    /// Level of the center speaker.
    CenterLevel(ChannelLevel),
    /// Center spread.
    CenterSpread(bool),
    /// Center width, from 0 to 7.
    CenterWidth(u8),
    /// Loudness management.
    LoudnessManagement(bool),
    /// Speaker virtualizer.
    SpeakerVirtualizer(bool),
    /// Graphic EQ.
    GraphicEq(bool),
    /// Headphone EQ.
    HeadphoneEq(bool),
    /// Cinema EQ.
    CinemaEq(bool),
    /// Neural:X.
    NeuralX(bool),
    /// Panorama.
    Panorama(bool),
    /// Dimension, from 0 to 6.
    Dimension(u8),
    /// Bass sync, from 0 to 16.
    BassSync(u8),
    /// Dialog control in dB, from 0 to 6.
    DialogControl(u8),
    /// Delay time in ms, from 0 to 300.
    DelayTime(u16),
    /// Audio delay in ms, from 0 to 500.
    AudioDelay(u16),
}

impl SoundSettingValue {
    /// Appends the command, without the line end, to `out`.
    ///
    /// `out` can have text added when this returns an error.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ValueOutOfRange`] for a number that the receiver does
    /// not accept.
    pub(crate) fn write(self, out: &mut String) -> Result<(), Error> {
        match self {
            Self::AudioRestorer(v) => code(out, "PSRSTR ", v.code()),
            Self::RoomSize(v) => code(out, "PSRSZ ", v.code()),
            Self::SurroundLevel(v) => code(out, "PSSURLEV ", v.code()),
            Self::DialogEnhancer(v) => code(out, "PSDEH ", v.code()),
            Self::Drc(v) => code(out, "PSDRC ", v.code()),
            Self::Auro3dMode(v) => code(out, "PSAUROMODE ", v.code()),
            Self::AuroMaticPreset(v) => code(out, "PSAUROPR ", v.code()),
            Self::AuroMaticStrength(v) => digits(
                out,
                "PSAUROST ",
                "Auro-Matic strength",
                u16::from(v),
                MAX_AURO_STRENGTH,
                2,
            ),
            Self::EffectSpeakers(v) => code(out, "PSSP:", v.code()),
            Self::Mdax(v) => code(out, "PSMDAX ", v.code()),
            Self::DacFilter(v) => code(out, "PSDACFIL ", v.code()),
            Self::Imax(v) => code(out, "PSIMAX ", v.code()),
            Self::ImaxAudio(v) => code(out, "PSIMAXAUD ", v.code()),
            Self::ImaxSubwooferOutput(v) => code(out, "PSIMAXSWO ", v.code()),
            Self::ImaxSubwooferMode(on) => code(out, "PSIMAXSWM ", on_off_code(on)),
            Self::ImaxHighPass(hz) => hertz(
                out,
                "PSIMAXHPF ",
                "IMAX high pass filter",
                hz,
                &IMAX_HIGH_PASS_HZ,
            ),
            Self::ImaxLowPass(hz) => hertz(
                out,
                "PSIMAXLPF ",
                "IMAX low pass filter",
                hz,
                &IMAX_LOW_PASS_HZ,
            ),
            Self::Subwoofer(on) => code(out, "PSSWR ", on_off_code(on)),
            Self::SubwooferLevel { number, level } => {
                let name = match number {
                    1 => "PSSWL ",
                    2 => "PSSWL2 ",
                    3 => "PSSWL3 ",
                    4 => "PSSWL4 ",
                    _ => {
                        return Err(Error::ValueOutOfRange {
                            what: "subwoofer number",
                        });
                    }
                };
                code(out, name, level.code())
            }
            Self::LfeLevel(v) => digits(out, "PSLFE ", "LFE level", u16::from(v), MAX_LFE_LEVEL, 2),
            Self::CenterLevel(level) => code(out, "PSCLV ", level.code()),
            Self::CenterSpread(on) => code(out, "PSCES ", on_off_code(on)),
            Self::CenterWidth(v) => digits(
                out,
                "PSCEN ",
                "center width",
                u16::from(v),
                MAX_CENTER_WIDTH,
                2,
            ),
            Self::LoudnessManagement(on) => code(out, "PSLOM ", on_off_code(on)),
            Self::SpeakerVirtualizer(on) => code(out, "PSSPV ", on_off_code(on)),
            Self::GraphicEq(on) => code(out, "PSGEQ ", on_off_code(on)),
            Self::HeadphoneEq(on) => code(out, "PSHEQ ", on_off_code(on)),
            // No space before the value.
            Self::CinemaEq(on) => code(out, "PSCINEMA EQ.", on_off_code(on)),
            Self::NeuralX(on) => code(out, "PSNEURAL ", on_off_code(on)),
            Self::Panorama(on) => code(out, "PSPAN ", on_off_code(on)),
            Self::Dimension(v) => {
                digits(out, "PSDIM ", "dimension", u16::from(v), MAX_DIMENSION, 2)
            }
            Self::BassSync(v) => digits(out, "PSBSC ", "bass sync", u16::from(v), MAX_BASS_SYNC, 2),
            Self::DialogControl(v) => digits(
                out,
                "PSDIC ",
                "dialog control",
                u16::from(v),
                MAX_DIALOG_CONTROL,
                2,
            ),
            Self::DelayTime(v) => digits(out, "PSDEL ", "delay time", v, MAX_DELAY_TIME, 3),
            Self::AudioDelay(v) => digits(out, "PSDELAY ", "audio delay", v, MAX_AUDIO_DELAY, 3),
        }
    }
}

#[allow(clippy::unnecessary_wraps)] // Same type as the number writers, so each arm is one call.
fn code(out: &mut String, start: &str, value: &str) -> Result<(), Error> {
    out.push_str(start);
    out.push_str(value);
    Ok(())
}

const fn on_off_code(on: bool) -> &'static str {
    if on { "ON" } else { "OFF" }
}

/// Writes a number with leading zeros.
fn digits(
    out: &mut String,
    start: &str,
    what: &'static str,
    value: u16,
    max: u16,
    width: usize,
) -> Result<(), Error> {
    if value > max {
        return Err(Error::ValueOutOfRange { what });
    }
    out.push_str(start);
    // Writing to a `String` cannot fail.
    let _ = write!(out, "{value:0width$}");
    Ok(())
}

/// Writes a frequency that must be one of the allowed values, with three
/// digits.
fn hertz(
    out: &mut String,
    start: &str,
    what: &'static str,
    hz: u16,
    allowed: &[u16],
) -> Result<(), Error> {
    if !allowed.contains(&hz) {
        return Err(Error::ValueOutOfRange { what });
    }
    digits(out, start, what, hz, u16::MAX, 3)
}

/// Sets the value if there is one. `None` if there is none. Otherwise returns
/// true if the value was different.
fn put<T: PartialEq>(slot: &mut Option<T>, value: Option<T>) -> Option<bool> {
    let value = value?;
    let changed = slot.as_ref() != Some(&value);
    *slot = Some(value);
    Some(changed)
}

fn on_off(text: &str) -> Option<bool> {
    match text {
        "ON" => Some(true),
        "OFF" => Some(false),
        _ => None,
    }
}

/// A number written with digits, not more than `max`.
fn number<T: TryFrom<u16>>(text: &str, max: u16) -> Option<T> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: u16 = text.parse().ok().filter(|value| *value <= max)?;
    T::try_from(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_values() {
        let mut settings = SoundSettings::default();
        assert!(settings.apply("RSTR", "LOW"));
        assert!(!settings.apply("RSTR", "LOW  "));
        assert!(settings.apply("RSTR", "OFF"));
        assert_eq!(settings.audio_restorer, Some(AudioRestorer::Off));
        assert!(settings.apply("RSZ", "ML"));
        assert!(settings.apply("DEH", "HIGH"));
        assert!(settings.apply("SURLEV", "HEV"));
        assert!(settings.apply("DIM", "03"));
        assert!(settings.apply("CEN", "07"));
        assert!(!settings.apply("CEW", "07"));
        assert!(settings.apply("CEW", "02"));
        assert_eq!(settings.center_width, Some(2));
        assert_eq!(
            settings.surround_level,
            Some(SurroundLevelCompensation::Heavy)
        );
        assert_eq!(settings.dimension, Some(3));
        assert!(settings.apply("DRC", "HI"));
        assert!(settings.apply("AUROMODE", "EXP"));
        assert!(settings.apply("AUROPR", "SPE"));
        assert!(settings.apply("SP:", "HF"));
        assert!(settings.apply("IMAX", "AUTO"));
        assert!(settings.apply("IMAXAUD", "MANUAL"));
        assert!(settings.apply("IMAXSWO", "L+M"));
        assert_eq!(settings.room_size, Some(RoomSize::MediumLarge));
        assert_eq!(settings.dialog_enhancer, Some(DialogEnhancerLevel::High));
        assert_eq!(settings.drc, Some(Drc::High));
        assert_eq!(settings.auro_3d_mode, Some(Auro3dMode::ChannelExpansion));
        assert_eq!(settings.auro_matic_preset, Some(AuroMatic3dPreset::Speech));
        assert_eq!(
            settings.effect_speakers,
            Some(EffectSpeakerSelection::HeightAndFloor)
        );
        assert_eq!(settings.imax, Some(ImaxMode::Auto));
        assert_eq!(settings.imax_audio, Some(ImaxAudio::Manual));
        assert_eq!(
            settings.imax_subwoofer_output,
            Some(ImaxSubwooferOutput::LfeAndMain)
        );
        assert_eq!(settings.other, BTreeMap::new());
    }

    #[test]
    fn on_off_values() {
        let mut settings = SoundSettings::default();
        for name in [
            "IMAXSWM", "SWR", "CES", "LOM", "SPV", "GEQ", "HEQ", "NEURAL", "PAN",
        ] {
            assert!(settings.apply(name, "ON"), "{name}");
        }
        assert!(settings.apply("CINEMA", "EQ.OFF"));
        assert!(!settings.apply("CINEMA", "EQ.OFF"));
        assert_eq!(settings.cinema_eq, Some(false));
        assert_eq!(settings.imax_subwoofer_mode, Some(true));
        assert_eq!(settings.subwoofer, Some(true));
        assert_eq!(settings.center_spread, Some(true));
        assert_eq!(settings.loudness_management, Some(true));
        assert_eq!(settings.speaker_virtualizer, Some(true));
        assert_eq!(settings.graphic_eq, Some(true));
        assert_eq!(settings.headphone_eq, Some(true));
        assert_eq!(settings.neural_x, Some(true));
        assert_eq!(settings.panorama, Some(true));
    }

    #[test]
    fn numbers_and_levels() {
        let mut settings = SoundSettings::default();
        assert!(settings.apply("BSC", "10"));
        assert!(settings.apply("LFE", "03"));
        assert!(settings.apply("DIC", "06"));
        assert!(settings.apply("AUROST", "15"));
        assert!(settings.apply("DEL", "300"));
        assert!(settings.apply("DELAY", "200"));
        assert!(settings.apply("IMAXHPF", "080"));
        assert!(settings.apply("IMAXLPF", "120"));
        assert!(settings.apply("SWL", "495"));
        assert!(settings.apply("SWL2", "48"));
        assert!(settings.apply("CLV", "505"));
        assert_eq!(settings.center_level.map(ChannelLevel::db), Some(0.5));
        assert_eq!(settings.bass_sync, Some(10));
        assert_eq!(settings.lfe_level, Some(3));
        assert_eq!(settings.dialog_control, Some(6));
        assert_eq!(settings.auro_matic_strength, Some(15));
        assert_eq!(settings.delay_time, Some(300));
        assert_eq!(settings.audio_delay, Some(200));
        assert_eq!(settings.imax_high_pass, Some(80));
        assert_eq!(settings.imax_low_pass, Some(120));
        assert_eq!(
            settings.subwoofer_levels[0].map(ChannelLevel::db),
            Some(-0.5)
        );
        assert_eq!(
            settings.subwoofer_levels[1].map(ChannelLevel::db),
            Some(-2.0)
        );
        assert_eq!(settings.subwoofer_levels[2], None);
        assert_eq!(settings.other, BTreeMap::new());
    }

    #[test]
    fn unknown_names_and_values_are_kept_as_text() {
        let mut settings = SoundSettings::default();
        // A name that is not known.
        assert!(settings.apply("XYZ", "HEV"));
        assert!(!settings.apply("XYZ", "HEV"));
        assert!(settings.apply("XYZ", "LIT"));
        // A known name with a value that is not known: the typed value stays.
        assert!(settings.apply("RSTR", "LOW"));
        assert!(settings.apply("RSTR", "ULTRA"));
        assert!(settings.apply("BSC", "99"));
        assert!(settings.apply("SWL", "00"));
        assert_eq!(settings.audio_restorer, Some(AudioRestorer::Low));
        assert_eq!(settings.bass_sync, None);
        assert_eq!(settings.other["XYZ"], "LIT");
        assert_eq!(settings.other["RSTR"], "ULTRA");
        assert_eq!(settings.other["BSC"], "99");
        assert_eq!(settings.other["SWL"], "00");
    }

    #[test]
    fn bad_text_is_ignored() {
        let mut settings = SoundSettings::default();
        for (name, value) in [
            ("RSTR", "?"),
            ("RSTR", ""),
            ("", "x"),
            ("A-B", "x"),
            ("SOMETHINGVERYLONGNAME", "1"),
        ] {
            assert!(!settings.apply(name, value), "{name} {value}");
        }
        assert_eq!(settings, SoundSettings::default());
    }

    #[test]
    fn setter_texts() {
        let level = |half_db| ChannelLevel::from_half_db(half_db).unwrap();
        let text = |value: SoundSettingValue| {
            let mut out = String::new();
            value.write(&mut out).map(|()| out)
        };
        let cases = [
            (
                SoundSettingValue::AudioRestorer(AudioRestorer::High),
                "PSRSTR HI",
            ),
            (
                SoundSettingValue::RoomSize(RoomSize::MediumLarge),
                "PSRSZ ML",
            ),
            (
                SoundSettingValue::SurroundLevel(SurroundLevelCompensation::Heavy),
                "PSSURLEV HEV",
            ),
            (
                SoundSettingValue::DialogEnhancer(DialogEnhancerLevel::Medium),
                "PSDEH MED",
            ),
            (SoundSettingValue::Drc(Drc::Auto), "PSDRC AUTO"),
            (
                SoundSettingValue::Auro3dMode(Auro3dMode::Direct),
                "PSAUROMODE DRCT",
            ),
            (
                SoundSettingValue::AuroMaticPreset(AuroMatic3dPreset::Large),
                "PSAUROPR LAR",
            ),
            (SoundSettingValue::AuroMaticStrength(5), "PSAUROST 05"),
            (
                SoundSettingValue::EffectSpeakers(EffectSpeakerSelection::Floor),
                "PSSP:FL",
            ),
            (SoundSettingValue::Mdax(Mdax::High), "PSMDAX HI"),
            (
                SoundSettingValue::DacFilter(DacFilter::Mode2),
                "PSDACFIL MODE2",
            ),
            (SoundSettingValue::Imax(ImaxMode::Auto), "PSIMAX AUTO"),
            (
                SoundSettingValue::ImaxAudio(ImaxAudio::Manual),
                "PSIMAXAUD MANUAL",
            ),
            (
                SoundSettingValue::ImaxSubwooferOutput(ImaxSubwooferOutput::LfeAndMain),
                "PSIMAXSWO L+M",
            ),
            (SoundSettingValue::ImaxSubwooferMode(true), "PSIMAXSWM ON"),
            (SoundSettingValue::ImaxHighPass(80), "PSIMAXHPF 080"),
            (SoundSettingValue::ImaxLowPass(250), "PSIMAXLPF 250"),
            (SoundSettingValue::Subwoofer(false), "PSSWR OFF"),
            (
                SoundSettingValue::SubwooferLevel {
                    number: 1,
                    level: level(-1),
                },
                "PSSWL 495",
            ),
            (
                SoundSettingValue::SubwooferLevel {
                    number: 4,
                    level: level(0),
                },
                "PSSWL4 50",
            ),
            (SoundSettingValue::LfeLevel(3), "PSLFE 03"),
            (SoundSettingValue::CenterLevel(level(2)), "PSCLV 51"),
            (SoundSettingValue::CenterSpread(true), "PSCES ON"),
            (SoundSettingValue::CenterWidth(7), "PSCEN 07"),
            (SoundSettingValue::LoudnessManagement(false), "PSLOM OFF"),
            (SoundSettingValue::SpeakerVirtualizer(true), "PSSPV ON"),
            (SoundSettingValue::GraphicEq(true), "PSGEQ ON"),
            (SoundSettingValue::HeadphoneEq(false), "PSHEQ OFF"),
            (SoundSettingValue::CinemaEq(true), "PSCINEMA EQ.ON"),
            (SoundSettingValue::NeuralX(true), "PSNEURAL ON"),
            (SoundSettingValue::Panorama(false), "PSPAN OFF"),
            (SoundSettingValue::Dimension(3), "PSDIM 03"),
            (SoundSettingValue::BassSync(16), "PSBSC 16"),
            (SoundSettingValue::DialogControl(6), "PSDIC 06"),
            (SoundSettingValue::DelayTime(5), "PSDEL 005"),
            (SoundSettingValue::AudioDelay(500), "PSDELAY 500"),
        ];
        for (value, expected) in cases {
            assert_eq!(text(value).as_deref(), Ok(expected), "{value:?}");
        }
    }

    #[test]
    fn setters_reject_numbers_out_of_range() {
        let level = ChannelLevel::from_half_db(0).unwrap();
        for value in [
            SoundSettingValue::AuroMaticStrength(16),
            SoundSettingValue::ImaxHighPass(70),
            SoundSettingValue::ImaxLowPass(40),
            SoundSettingValue::SubwooferLevel { number: 0, level },
            SoundSettingValue::SubwooferLevel { number: 5, level },
            SoundSettingValue::LfeLevel(11),
            SoundSettingValue::CenterWidth(8),
            SoundSettingValue::Dimension(7),
            SoundSettingValue::BassSync(17),
            SoundSettingValue::DialogControl(7),
            SoundSettingValue::DelayTime(301),
            SoundSettingValue::AudioDelay(501),
        ] {
            let mut out = String::new();
            assert!(
                matches!(value.write(&mut out), Err(Error::ValueOutOfRange { .. })),
                "{value:?}"
            );
        }
    }

    #[test]
    fn the_other_table_has_a_limit() {
        let mut settings = SoundSettings::default();
        for n in 0..300 {
            settings.apply(&format!("X{n}"), "1");
        }
        assert_eq!(settings.other.len(), MAX_OTHER);
    }
}
