//! Commands and state of the tuner.

use core::fmt;
use core::fmt::Write as _;

use crate::brand::Brand;
use crate::error::Error;
use crate::speaker_tables::root;
use crate::values::{DabDrc, DabStationOrder, HdRadioBand, HdTuningMode, TunerBand, TunerMode};
use crate::xml::Node;

/// Queries for the frequency, the preset and the tuning mode. A receiver
/// answers them for the tuner source only.
pub const TUNER_QUERIES: [&str; 3] = ["TFAN?", "TPAN?", "TMAN?"];

/// Queries for the HD Radio tuner: the status, the frequency, the preset and
/// the band and tuning mode. A receiver answers them for the HD Radio source
/// only.
pub const HD_RADIO_QUERIES: [&str; 4] = ["HD?", "TFHD?", "TPHD?", "TMHD?"];

/// Queries for the DAB tuner: the status lines, the dynamic range control and
/// the station order. Only DAB models answer them.
pub const DAB_QUERIES: [&str; 3] = ["DA ?", "DADRC ?", "DAODR ?"];

/// Query for the RDS name of the station. Denon receivers answer
/// `TFANNAME`, Marantz receivers `TPANNAME`.
#[must_use]
pub const fn station_name_query(brand: Brand) -> &'static str {
    match brand {
        Brand::Denon => "TFANNAME?",
        Brand::Marantz => "TPANNAME?",
    }
}

/// Query for the names and frequencies of the tuner presets. The answers
/// fill [`TunerState::presets`].
pub const TUNER_PRESETS_QUERY: &str = "OPTPN ?";

/// Path of the status page of the tuner.
pub const TUNER_STATUS_PATH: &str = "/goform/formTuner_TunerXml.xml";

/// Highest preset number of the tuner.
const MAX_PRESET: u8 = 56;

/// Highest frequency in hundredths: six digits.
const MAX_HUNDREDTHS: u32 = 999_999;

/// Highest HD Radio signal level.
const MAX_SIGNAL_LEVEL: u8 = 6;

/// Highest HD Radio multicast channel. Channel 0 is the analog broadcast.
const MAX_MULTICAST: u8 = 8;

/// Frequency of the tuner as the receiver reports it: six digits written as
/// `****.**`. Values above `050000` are AM in kHz, the others FM in MHz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TunerFrequency {
    hundredths: u32,
}

impl TunerFrequency {
    /// The frequency in hundredths: of kHz for AM, of MHz for FM. For
    /// example 8750 for 87.50 MHz or 105000 for 1050.00 kHz. `None` for 0 and
    /// for more than six digits.
    #[must_use]
    pub const fn from_hundredths(hundredths: u32) -> Option<Self> {
        if hundredths == 0 || hundredths > MAX_HUNDREDTHS {
            None
        } else {
            Some(Self { hundredths })
        }
    }

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

impl TunerFrequency {
    /// Reads the frequency of the status page, for example `106.80`. `None`
    /// for text that is not a frequency and for zero, which the page shows
    /// when the tuner is not tuned.
    fn from_page(text: &str) -> Option<Self> {
        let (whole, fraction) = text.trim().split_once('.')?;
        let digits = |text: &str| text.bytes().all(|byte| byte.is_ascii_digit());
        if whole.is_empty() || fraction.len() != 2 || !digits(whole) || !digits(fraction) {
            return None;
        }
        let hundredths =
            whole.parse::<u32>().ok()?.checked_mul(100)? + fraction.parse::<u32>().ok()?;
        (hundredths > 0).then_some(Self { hundredths })
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
    /// The receiver reports presets as A1 to G8 (`true`) or as 01 to 56
    /// (`false`). `None` until it reports a preset.
    pub presets_lettered: Option<bool>,
    /// Name of the station from RDS, in Europe and Asia Pacific models.
    pub station_name: Option<String>,
    /// The HD Radio tuner.
    pub hd: HdRadioState,
    /// The DAB tuner.
    pub dab: DabState,
    /// The tuner presets, from the answers to [`TUNER_PRESETS_QUERY`]. The
    /// first is preset 1. A preset that the receiver has not reported has no
    /// values.
    pub presets: Vec<TunerPreset>,
}

/// What the DAB tuner reports, on DAB models. A value is `None` until the
/// receiver reports it. The texts are as the receiver sends them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct DabState {
    /// Long name of the station (`DASTN`).
    pub station_name: Option<String>,
    /// Program type (`DAPTY`).
    pub program_type: Option<String>,
    /// Ensemble label (`DAENL`).
    pub ensemble: Option<String>,
    /// Frequency (`DAFRQ`).
    pub frequency: Option<String>,
    /// Signal quality (`DAQUA`).
    pub quality: Option<String>,
    /// Audio information (`DAINF`).
    pub audio_info: Option<String>,
    /// Dynamic range control.
    pub drc: Option<DabDrc>,
    /// Order of the stations.
    pub station_order: Option<DabStationOrder>,
}

impl DabState {
    /// Applies the text of a `DA` event without `DA`, for example
    /// `STN BBC Radio 2` or `DRC HLF`. Returns `None` if it is not a value
    /// that this library knows, and whether the state changed otherwise.
    pub(crate) fn apply(&mut self, parameter: &str) -> Option<bool> {
        let (name, value) = parameter.split_at_checked(3)?;
        let value = value.trim();
        let text = match name {
            "DRC" => return Some(replace(&mut self.drc, Some(DabDrc::from_code(value)?))),
            "ODR" => {
                let order = DabStationOrder::from_code(value)?;
                return Some(replace(&mut self.station_order, Some(order)));
            }
            "STN" => &mut self.station_name,
            "PTY" => &mut self.program_type,
            "ENL" => &mut self.ensemble,
            "FRQ" => &mut self.frequency,
            "QUA" => &mut self.quality,
            "INF" => &mut self.audio_info,
            _ => return None,
        };
        // `?` is the query, not a value.
        if value == "?" {
            return None;
        }
        Some(match text {
            _ if value.is_empty() => text.take().is_some(),
            Some(current) if current == value => false,
            // Reuses the buffer.
            Some(current) => {
                current.clear();
                current.push_str(value);
                true
            }
            None => {
                *text = Some(value.to_owned());
                true
            }
        })
    }
}

/// A tuner preset, as the answer to [`TUNER_PRESETS_QUERY`] tells it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TunerPreset {
    /// Name. `None` if the preset has no name.
    pub name: Option<String>,
    /// Frequency.
    pub frequency: Option<TunerFrequency>,
}

/// What the HD Radio tuner reports, in North America models. A value is
/// `None` until the receiver reports it. The title, artist, album and short
/// station name are in [`NowPlaying`](crate::NowPlaying).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HdRadioState {
    /// Band.
    pub band: Option<HdRadioBand>,
    /// Tuning mode.
    pub mode: Option<HdTuningMode>,
    /// Frequency.
    pub frequency: Option<TunerFrequency>,
    /// Multicast channel from 1 to 8, or 0 for the analog broadcast.
    pub multicast_channel: Option<u8>,
    /// Number of multicast channels of the station.
    pub multicast_count: Option<u8>,
    /// Preset number from 1 to 56. `None` if the tuner is not on a preset.
    pub preset: Option<u8>,
    /// The receiver reports HD Radio presets as A1 to G8 (`true`) or as 01
    /// to 56 (`false`). `None` until it reports a preset.
    pub presets_lettered: Option<bool>,
    /// Signal level from 0 to 6.
    pub signal_level: Option<u8>,
    /// The station plays digital (`true`) or analog (`false`).
    pub digital: Option<bool>,
    /// Long name of the station.
    pub long_station_name: Option<String>,
    /// Program type, for example `Rock`.
    pub program_type: Option<String>,
    /// Genre.
    pub genre: Option<String>,
}

impl HdRadioState {
    /// Applies the text of an `HD` event that is not about what plays, for
    /// example `SIG LEV 4` or `MODE DIGITAL`. Returns `None` if it is not a
    /// value that this library knows, and whether the state changed
    /// otherwise.
    pub(crate) fn apply_status(&mut self, parameter: &str) -> Option<bool> {
        if let Some(level) = parameter.strip_prefix("SIG LEV ") {
            let level = digit(level).filter(|level| *level <= MAX_SIGNAL_LEVEL)?;
            return Some(replace(&mut self.signal_level, Some(level)));
        }
        if let Some(channel) = parameter.strip_prefix("MLT CURRCH ") {
            let channel = digit(channel).filter(|channel| *channel <= MAX_MULTICAST)?;
            return Some(replace(&mut self.multicast_channel, Some(channel)));
        }
        if let Some(count) = parameter.strip_prefix("MLT CAST CH ") {
            return Some(replace(&mut self.multicast_count, Some(digit(count)?)));
        }
        if let Some(mode) = parameter.strip_prefix("MODE ") {
            let digital = match mode.trim() {
                "DIGITAL" => true,
                "ANALOG" => false,
                _ => return None,
            };
            return Some(replace(&mut self.digital, Some(digital)));
        }
        let (field, text) = [
            ("STL NAME", &mut self.long_station_name),
            ("PTY", &mut self.program_type),
            ("GENRE", &mut self.genre),
        ]
        .into_iter()
        .find_map(|(prefix, field)| Some((field, parameter.strip_prefix(prefix)?)))?;
        let text = text.trim();
        if field.as_deref() == Some(text) || (field.is_none() && text.is_empty()) {
            return Some(false);
        }
        *field = (!text.is_empty()).then(|| text.to_owned());
        Some(true)
    }

    /// Applies the text of a `TF` event after `HD`, for example `008750`,
    /// `MC2` or `008750MC2`.
    fn apply_frequency(&mut self, rest: &str) -> Option<bool> {
        let (digits, channel) = match rest.split_once("MC") {
            Some((digits, channel)) => (digits, Some(channel)),
            None => (rest, None),
        };
        let frequency = if digits.is_empty() {
            None
        } else {
            Some(TunerFrequency::from_digits(digits)?)
        };
        let channel = match channel {
            Some(channel) => Some(digit(channel).filter(|channel| *channel <= MAX_MULTICAST)?),
            None => None,
        };
        if frequency.is_none() && channel.is_none() {
            return None;
        }
        let frequency = frequency.is_some_and(|f| replace(&mut self.frequency, Some(f)));
        let channel = channel.is_some_and(|c| replace(&mut self.multicast_channel, Some(c)));
        Some(frequency | channel)
    }
}

/// Reads one digit.
fn digit(text: &str) -> Option<u8> {
    match text.trim().as_bytes() {
        [byte @ b'0'..=b'9'] => Some(byte - b'0'),
        _ => None,
    }
}

impl TunerState {
    /// Applies the text of a `TF` event, for example `AN105000` or
    /// `ANNAME BBC R2 `. Returns `None` if it is not a value that this library
    /// knows, and whether the state changed otherwise.
    pub(crate) fn apply_frequency(&mut self, parameter: &str) -> Option<bool> {
        if let Some(rest) = parameter.strip_prefix("HD") {
            return self.hd.apply_frequency(rest);
        }
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

    /// Applies the text of an `OPTPN` answer without `OPTPN`, for example
    /// `03KEXP     009250`: the preset number, the name and the frequency.
    pub(crate) fn apply_preset_name(&mut self, rest: &str) -> Option<bool> {
        let index = usize::from(parse_preset(rest.get(..2)?)? - 1);
        let rest = rest[2..].trim_end();
        // The frequency is the last six characters. They are ASCII digits,
        // so the start of them is a char boundary.
        let start = rest.len().checked_sub(6)?;
        if !rest.as_bytes()[start..].iter().all(u8::is_ascii_digit) {
            return None;
        }
        let frequency = TunerFrequency::from_digits(&rest[start..]);
        let name = rest[..start].trim();
        let is_new = self.presets.len() <= index;
        if is_new {
            self.presets.resize(index + 1, TunerPreset::default());
        }
        let preset = &mut self.presets[index];
        let same = preset.frequency == frequency
            && preset.name.as_deref() == Some(name).filter(|n| !n.is_empty());
        if same {
            return Some(is_new);
        }
        preset.frequency = frequency;
        preset.name = (!name.is_empty()).then(|| name.to_owned());
        Some(true)
    }

    /// Applies the text of a `TP` event, for example `AN01`, `ANA1` or
    /// `ANOFF`.
    pub(crate) fn apply_preset(&mut self, parameter: &str) -> Option<bool> {
        // Marantz receivers send the RDS name as `TPANNAME`.
        if parameter.starts_with("ANNAME") {
            return self.apply_frequency(parameter);
        }
        let (slot, lettered, rest) = if let Some(rest) = parameter.strip_prefix("HD") {
            (&mut self.hd.preset, &mut self.hd.presets_lettered, rest)
        } else {
            let rest = parameter.strip_prefix("AN")?;
            (&mut self.preset, &mut self.presets_lettered, rest)
        };
        if rest == "OFF" {
            return Some(replace(slot, None));
        }
        let preset = parse_preset(rest)?;
        let form = rest.as_bytes()[0].is_ascii_uppercase();
        let form_changed = replace(lettered, Some(form));
        Some(replace(slot, Some(preset)) | form_changed)
    }

    /// Applies the text of a `TM` event, for example `ANFM` or `ANAUTO`.
    pub(crate) fn apply_band_or_mode(&mut self, parameter: &str) -> Option<bool> {
        if let Some(code) = parameter.strip_prefix("HD") {
            return if let Some(band) = HdRadioBand::from_code(code) {
                Some(replace(&mut self.hd.band, Some(band)))
            } else {
                let mode = HdTuningMode::from_code(code)?;
                Some(replace(&mut self.hd.mode, Some(mode)))
            };
        }
        let code = parameter.strip_prefix("AN")?;
        if let Some(band) = TunerBand::from_code(code) {
            Some(replace(&mut self.band, Some(band)))
        } else {
            let mode = TunerMode::from_code(code)?;
            Some(replace(&mut self.mode, Some(mode)))
        }
    }
}

/// What the status page of the tuner tells. A value is `None` if the page has
/// no value, for example when the source is not the tuner.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TunerPage {
    /// Band. A DAB receiver writes `DA`.
    pub band: Option<TunerBand>,
    /// Frequency.
    pub frequency: Option<TunerFrequency>,
}

impl TunerPage {
    /// Reads the status page of the tuner.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML or is an HTML page.
    pub fn parse(body: &[u8]) -> Result<Self, Error> {
        let node = root(body)?.node();
        Ok(Self {
            band: value_of(node, "Band").and_then(|band| match band.trim() {
                "AM" => Some(TunerBand::Am),
                "FM" => Some(TunerBand::Fm),
                "DA" | "DAB" => Some(TunerBand::Dab),
                _ => None,
            }),
            frequency: value_of(node, "Frequency")
                .and_then(|frequency| TunerFrequency::from_page(&frequency)),
        })
    }
}

impl TunerState {
    /// Stores the values of the status page. A value that the page does not
    /// have keeps what telnet reported. Returns true if the state changed.
    pub(crate) fn apply_page(&mut self, page: TunerPage) -> bool {
        let band = page
            .band
            .is_some_and(|band| replace(&mut self.band, Some(band)));
        let frequency = page
            .frequency
            .is_some_and(|frequency| replace(&mut self.frequency, Some(frequency)));
        band | frequency
    }
}

/// Text of the `value` element in the element `tag`.
fn value_of(node: Node<'_>, tag: &str) -> Option<String> {
    let value = node.child(tag)?.node().child("value")?;
    Some(value.node().text()?.into_owned())
}

/// Reads a preset as two digits, `01` to `56`, or as a letter and a digit,
/// `A1` to `G8`.
fn parse_preset(text: &str) -> Option<u8> {
    if let [letter @ b'A'..=b'G', digit @ b'1'..=b'8'] = text.as_bytes() {
        return Some((letter - b'A') * 8 + (digit - b'0'));
    }
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
///
/// The HD Radio commands are for receivers with an HD Radio tuner only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TunerCommand {
    /// Tunes to the next frequency.
    FrequencyUp,
    /// Tunes to a frequency.
    Frequency(TunerFrequency),
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
    /// Leaves the preset. North America models only.
    PresetOff,
    /// Goes to a preset, from 1 to 56.
    Preset(u8),
    /// Stores the station in a preset, from 1 to 56.
    StorePreset(u8),
    /// Goes to a preset, from 1 to 56, in the form A1 to G8 of older
    /// receivers, for example Marantz of 2011 and some Denon of 2011 to 2013.
    /// Preset 1 is A1, 9 is B1 and 56 is G8. Receivers that report presets
    /// as A1 to G8 need this form. [`TunerState::presets_lettered`] says
    /// which form the receiver uses.
    LetteredPreset(u8),
    /// Stores the station in a preset, from 1 to 56, in the form A1 to G8.
    /// See [`TunerCommand::LetteredPreset`].
    StoreLetteredPreset(u8),
    /// Tunes the HD Radio tuner to the next frequency.
    HdFrequencyUp,
    /// Tunes the HD Radio tuner to the previous frequency.
    HdFrequencyDown,
    /// Selects an HD Radio multicast channel, from 1 to 8, or 0 for the
    /// analog broadcast.
    HdMulticast(u8),
    /// Tunes the HD Radio tuner to a frequency.
    HdFrequency(TunerFrequency),
    /// Tunes the HD Radio tuner to a frequency and selects a multicast
    /// channel, from 1 to 8, or 0 for the analog broadcast.
    HdFrequencyMulticast(TunerFrequency, u8),
    /// Selects the band of the HD Radio tuner.
    HdBand(HdRadioBand),
    /// Selects the tuning mode of the HD Radio tuner.
    HdMode(HdTuningMode),
    /// Goes to the next HD Radio preset.
    HdPresetUp,
    /// Goes to the previous HD Radio preset.
    HdPresetDown,
    /// Goes to an HD Radio preset, from 1 to 56.
    HdPreset(u8),
    /// Stores the HD Radio station in a preset, from 1 to 56.
    HdStorePreset(u8),
    /// Goes to an HD Radio preset, from 1 to 56, in the form A1 to G8. See
    /// [`TunerCommand::LetteredPreset`].
    HdLetteredPreset(u8),
    /// Stores the HD Radio station in a preset, from 1 to 56, in the form A1
    /// to G8. See [`TunerCommand::LetteredPreset`].
    HdStoreLetteredPreset(u8),
    /// Sets the dynamic range control of the DAB tuner. DAB models only.
    DabDrc(DabDrc),
    /// Sets the order of the DAB stations. DAB models only.
    DabStationOrder(DabStationOrder),
}

impl TunerCommand {
    /// Appends the command text, without the line end.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ValueOutOfRange`] for a preset that is not from 1 to
    /// 56, or a multicast channel above 8. `out` is not changed then.
    pub(crate) fn write(self, out: &mut String) -> Result<(), Error> {
        match self {
            Self::FrequencyUp => out.push_str("TFANUP"),
            Self::Frequency(frequency) => {
                let _ = write!(out, "TFAN{:06}", frequency.hundredths);
            }
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
            Self::PresetOff => out.push_str("TPANOFF"),
            Self::Preset(number) => write_preset(out, "TPAN", number)?,
            Self::StorePreset(number) => write_preset(out, "TPANMEM", number)?,
            Self::LetteredPreset(number) => write_lettered_preset(out, "TPAN", number)?,
            Self::StoreLetteredPreset(number) => {
                write_lettered_preset(out, "TPANMEM", number)?;
            }
            Self::HdFrequencyUp => out.push_str("TFHDUP"),
            Self::HdFrequencyDown => out.push_str("TFHDDOWN"),
            Self::HdMulticast(channel) => {
                check_multicast(channel)?;
                let _ = write!(out, "TFHDMC{channel}");
            }
            Self::HdFrequency(frequency) => {
                let _ = write!(out, "TFHD{:06}", frequency.hundredths);
            }
            Self::HdFrequencyMulticast(frequency, channel) => {
                check_multicast(channel)?;
                let _ = write!(out, "TFHD{:06}MC{channel}", frequency.hundredths);
            }
            Self::HdBand(band) => {
                out.push_str("TMHD");
                out.push_str(band.code());
            }
            Self::HdMode(mode) => {
                out.push_str("TMHD");
                out.push_str(mode.code());
            }
            Self::HdPresetUp => out.push_str("TPHDUP"),
            Self::HdPresetDown => out.push_str("TPHDDOWN"),
            Self::HdPreset(number) => write_preset(out, "TPHD", number)?,
            Self::HdStorePreset(number) => write_preset(out, "TPHDMEM", number)?,
            Self::HdLetteredPreset(number) => write_lettered_preset(out, "TPHD", number)?,
            Self::HdStoreLetteredPreset(number) => {
                write_lettered_preset(out, "TPHDMEM", number)?;
            }
            Self::DabDrc(drc) => {
                out.push_str("DADRC ");
                out.push_str(drc.code());
            }
            Self::DabStationOrder(order) => {
                out.push_str("DAODR ");
                out.push_str(order.code());
            }
        }
        Ok(())
    }
}

const fn check_multicast(channel: u8) -> Result<(), Error> {
    if channel > MAX_MULTICAST {
        return Err(Error::ValueOutOfRange {
            what: "HD Radio multicast channel",
        });
    }
    Ok(())
}

/// Writes a preset from 1 to 56 as A1 to G8: eight presets for each letter.
fn write_lettered_preset(out: &mut String, start: &str, number: u8) -> Result<(), Error> {
    check_preset(number)?;
    let index = number - 1;
    out.push_str(start);
    out.push(char::from(b'A' + index / 8));
    out.push(char::from(b'1' + index % 8));
    Ok(())
}

const fn check_preset(number: u8) -> Result<(), Error> {
    if number == 0 || number > MAX_PRESET {
        return Err(Error::ValueOutOfRange {
            what: "tuner preset",
        });
    }
    Ok(())
}

fn write_preset(out: &mut String, start: &str, number: u8) -> Result<(), Error> {
    check_preset(number)?;
    out.push_str(start);
    // Writing to a `String` cannot fail.
    let _ = write!(out, "{number:02}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAB: &[u8] = include_bytes!("../tests/data/xml/M-CR603-formTuner_TunerXml.xml");
    const FM: &[u8] = include_bytes!("../tests/data/xml/Own-formTuner_TunerXml-fm.xml");
    const IDLE: &[u8] = include_bytes!("../tests/data/xml/AVR-3312-formTuner_TunerXml.xml");

    #[test]
    fn tuner_pages() {
        let fm = TunerPage::parse(FM).unwrap();
        assert_eq!(fm.band, Some(TunerBand::Fm));
        assert_eq!(fm.frequency.unwrap().hundredths(), 8750);
        let dab = TunerPage::parse(DAB).unwrap();
        assert_eq!(dab.band, Some(TunerBand::Dab));
        assert_eq!(dab.frequency.unwrap().hundredths(), 10680);
        // A tuner that is not tuned shows no band and a frequency of zero.
        assert_eq!(TunerPage::parse(IDLE).unwrap(), TunerPage::default());
        assert!(TunerPage::parse(b"<html><body>no</body></html>").is_err());
    }

    #[test]
    fn page_frequencies() {
        let hundredths = |text| TunerFrequency::from_page(text).map(TunerFrequency::hundredths);
        assert_eq!(hundredths(" 87.50"), Some(8750));
        assert_eq!(hundredths("1050.00"), Some(105_000));
        for text in ["", " 0.00", "87.5", "87", "a.50", ".50", "87.5x", "-1.00"] {
            assert_eq!(hundredths(text), None, "{text:?}");
        }
    }

    #[test]
    fn a_page_keeps_what_it_does_not_have() {
        let mut state = TunerState::default();
        assert!(state.apply_frequency("AN105000").unwrap());
        assert!(!state.apply_page(TunerPage::default()));
        assert_eq!(state.frequency.unwrap().hundredths(), 105_000);
        assert!(state.apply_page(TunerPage::parse(FM).unwrap()));
        assert!(!state.apply_page(TunerPage::parse(FM).unwrap()));
        assert_eq!(state.frequency.unwrap().hundredths(), 8750);
        assert_eq!(state.band, Some(TunerBand::Fm));
    }

    fn text(command: TunerCommand) -> Result<String, Error> {
        let mut out = String::new();
        command.write(&mut out).map(|()| out)
    }

    #[test]
    fn texts() {
        let fm = TunerFrequency::from_hundredths(8750).unwrap();
        let am = TunerFrequency::from_hundredths(105_000).unwrap();
        let cases = [
            (TunerCommand::FrequencyUp, "TFANUP"),
            (TunerCommand::FrequencyDown, "TFANDOWN"),
            (TunerCommand::Frequency(fm), "TFAN008750"),
            (TunerCommand::Frequency(am), "TFAN105000"),
            (TunerCommand::Band(TunerBand::Fm), "TMANFM"),
            (TunerCommand::Band(TunerBand::Am), "TMANAM"),
            (TunerCommand::Band(TunerBand::Dab), "TMANDAB"),
            (TunerCommand::Mode(TunerMode::Auto), "TMANAUTO"),
            (TunerCommand::Mode(TunerMode::Manual), "TMANMANUAL"),
            (TunerCommand::PresetUp, "TPANUP"),
            (TunerCommand::PresetDown, "TPANDOWN"),
            (TunerCommand::PresetOff, "TPANOFF"),
            (TunerCommand::DabDrc(DabDrc::Half), "DADRC HLF"),
            (
                TunerCommand::DabStationOrder(DabStationOrder::Multiplex),
                "DAODR MLT",
            ),
            (TunerCommand::Preset(1), "TPAN01"),
            (TunerCommand::Preset(56), "TPAN56"),
            (TunerCommand::StorePreset(7), "TPANMEM07"),
            (TunerCommand::LetteredPreset(1), "TPANA1"),
            (TunerCommand::LetteredPreset(9), "TPANB1"),
            (TunerCommand::LetteredPreset(56), "TPANG8"),
            (TunerCommand::StoreLetteredPreset(16), "TPANMEMB8"),
            (TunerCommand::HdFrequencyUp, "TFHDUP"),
            (TunerCommand::HdFrequencyDown, "TFHDDOWN"),
            (TunerCommand::HdMulticast(0), "TFHDMC0"),
            (TunerCommand::HdMulticast(2), "TFHDMC2"),
            (TunerCommand::HdMulticast(8), "TFHDMC8"),
            (TunerCommand::HdFrequency(fm), "TFHD008750"),
            (TunerCommand::HdFrequency(am), "TFHD105000"),
            (TunerCommand::HdFrequencyMulticast(fm, 5), "TFHD008750MC5"),
            (TunerCommand::HdBand(HdRadioBand::Am), "TMHDAM"),
            (TunerCommand::HdBand(HdRadioBand::Fm), "TMHDFM"),
            (TunerCommand::HdMode(HdTuningMode::AutoHd), "TMHDAUTOHD"),
            (TunerCommand::HdMode(HdTuningMode::Auto), "TMHDAUTO"),
            (TunerCommand::HdMode(HdTuningMode::Manual), "TMHDMANUAL"),
            (
                TunerCommand::HdMode(HdTuningMode::AnalogAuto),
                "TMHDANAAUTO",
            ),
            (
                TunerCommand::HdMode(HdTuningMode::AnalogManual),
                "TMHDANAMANU",
            ),
            (TunerCommand::HdPresetUp, "TPHDUP"),
            (TunerCommand::HdPresetDown, "TPHDDOWN"),
            (TunerCommand::HdPreset(1), "TPHD01"),
            (TunerCommand::HdPreset(56), "TPHD56"),
            (TunerCommand::HdStorePreset(1), "TPHDMEM01"),
            (TunerCommand::HdLetteredPreset(10), "TPHDB2"),
            (TunerCommand::HdStoreLetteredPreset(56), "TPHDMEMG8"),
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
                TunerCommand::LetteredPreset(number),
                TunerCommand::StoreLetteredPreset(number),
                TunerCommand::HdPreset(number),
                TunerCommand::HdStorePreset(number),
                TunerCommand::HdLetteredPreset(number),
                TunerCommand::HdStoreLetteredPreset(number),
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
        assert_eq!(state.apply_preset("ANNAMEBBC_R2__"), Some(true));
        assert_eq!(state.station_name.as_deref(), Some("BBC R2"));
        assert_eq!(state.apply_preset("ANNAMEBBC_R2__"), Some(false));
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
    fn hd_multicast_channels_are_from_0_to_8() {
        let fm = TunerFrequency::from_hundredths(8750).unwrap();
        for channel in [9, 10, 255] {
            for command in [
                TunerCommand::HdMulticast(channel),
                TunerCommand::HdFrequencyMulticast(fm, channel),
            ] {
                assert_eq!(
                    text(command),
                    Err(Error::ValueOutOfRange {
                        what: "HD Radio multicast channel"
                    })
                );
            }
        }
    }

    #[test]
    fn frequencies_from_hundredths() {
        assert_eq!(
            TunerFrequency::from_hundredths(8750).map(TunerFrequency::hundredths),
            Some(8750)
        );
        assert_eq!(
            TunerFrequency::from_hundredths(999_999).map(TunerFrequency::hundredths),
            Some(999_999)
        );
        assert_eq!(TunerFrequency::from_hundredths(0), None);
        assert_eq!(TunerFrequency::from_hundredths(1_000_000), None);
    }

    #[test]
    fn hd_events_fill_the_hd_state() {
        let mut state = TunerState::default();
        assert_eq!(state.apply_frequency("HD008750"), Some(true));
        assert_eq!(state.apply_frequency("HD008750"), Some(false));
        assert_eq!(state.apply_frequency("HDMC2"), Some(true));
        assert_eq!(state.apply_frequency("HD010110MC3"), Some(true));
        assert_eq!(state.hd.frequency.unwrap().hundredths(), 10110);
        assert_eq!(state.hd.multicast_channel, Some(3));
        assert_eq!(state.apply_preset("HD07"), Some(true));
        assert_eq!(state.hd.preset, Some(7));
        assert_eq!(state.apply_preset("HDOFF"), Some(true));
        assert_eq!(state.hd.preset, None);
        assert_eq!(state.apply_band_or_mode("HDAM"), Some(true));
        assert_eq!(state.apply_band_or_mode("HDAUTOHD"), Some(true));
        assert_eq!(state.hd.band, Some(HdRadioBand::Am));
        assert_eq!(state.hd.mode, Some(HdTuningMode::AutoHd));
        // The analog values stay as they are.
        assert_eq!(state.frequency, None);
        assert_eq!(state.preset, None);
        assert_eq!(state.band, None);
        let hd = &mut state.hd;
        assert_eq!(hd.apply_status("SIG LEV 4"), Some(true));
        assert_eq!(hd.apply_status("SIG LEV 4"), Some(false));
        assert_eq!(hd.apply_status("MLT CURRCH 1"), Some(true));
        assert_eq!(hd.apply_status("MLT CAST CH 3"), Some(true));
        assert_eq!(hd.apply_status("MODE DIGITAL"), Some(true));
        assert_eq!(hd.apply_status("STL NAME Long Name  "), Some(true));
        assert_eq!(hd.apply_status("PTY Rock"), Some(true));
        assert_eq!(hd.apply_status("GENRE Jazz"), Some(true));
        assert_eq!(hd.apply_status("GENRE Jazz"), Some(false));
        assert_eq!(hd.signal_level, Some(4));
        assert_eq!(hd.multicast_channel, Some(1));
        assert_eq!(hd.multicast_count, Some(3));
        assert_eq!(hd.digital, Some(true));
        assert_eq!(hd.long_station_name.as_deref(), Some("Long Name"));
        assert_eq!(hd.program_type.as_deref(), Some("Rock"));
        assert_eq!(hd.genre.as_deref(), Some("Jazz"));
        assert_eq!(hd.apply_status("GENRE "), Some(true));
        assert_eq!(hd.genre, None);
        assert_eq!(hd.apply_status("GENRE "), Some(false));
    }

    #[test]
    fn preset_names_follow_answers() {
        let mut state = TunerState::default();
        assert_eq!(state.apply_preset_name("03KEXP     009250"), Some(true));
        assert_eq!(state.apply_preset_name("03KEXP     009250"), Some(false));
        assert_eq!(state.presets.len(), 3);
        assert_eq!(state.presets[0], TunerPreset::default());
        assert_eq!(state.presets[2].name.as_deref(), Some("KEXP"));
        assert_eq!(state.presets[2].frequency.unwrap().hundredths(), 9250);
        // An empty slot has a frequency but no name.
        assert_eq!(state.apply_preset_name("01         009210"), Some(true));
        assert_eq!(state.presets[0].name, None);
        assert_eq!(state.presets[0].frequency.unwrap().hundredths(), 9210);
        assert_eq!(state.apply_preset_name("01         009210"), Some(false));
        // Spaces after the frequency.
        assert_eq!(state.apply_preset_name("02WXYZ     010110  "), Some(true));
        assert_eq!(state.presets[1].name.as_deref(), Some("WXYZ"));
        for text in [
            "",
            "00KEXP     009250",
            "57KEXP     009250",
            "0xKEXP 009250",
            "03KEXP",
            "03KEXP     00925x",
            "03",
        ] {
            assert_eq!(state.apply_preset_name(text), None, "{text:?}");
        }
        assert_eq!(state.presets.len(), 3);
    }

    #[test]
    fn hd_events_that_are_not_values_change_nothing() {
        let mut state = TunerState::default();
        for text in ["HD", "HDUP", "HD0087", "HDMC9", "HDMC", "HD008750MC", "HD?"] {
            assert_eq!(state.apply_frequency(text), None, "{text:?}");
        }
        assert_eq!(state.apply_preset("ANA1"), Some(true));
        assert_eq!(state.preset, Some(1));
        assert_eq!(state.presets_lettered, Some(true));
        assert_eq!(state.apply_preset("ANG8"), Some(true));
        assert_eq!(state.preset, Some(56));
        // OFF keeps the form.
        assert_eq!(state.apply_preset("ANOFF"), Some(true));
        assert_eq!(state.presets_lettered, Some(true));
        // The same preset in the other form is a change.
        assert_eq!(state.apply_preset("AN56"), Some(true));
        assert_eq!(state.presets_lettered, Some(false));
        assert_eq!(state.apply_preset("AN56"), Some(false));
        assert_eq!(state.apply_preset("HDB2"), Some(true));
        assert_eq!(state.hd.preset, Some(10));
        assert_eq!(state.hd.presets_lettered, Some(true));
        state.preset = None;
        state.presets_lettered = None;
        state.hd = HdRadioState::default();
        for text in ["ANA0", "ANA9", "ANH1", "ANa1"] {
            assert_eq!(state.apply_preset(text), None, "{text:?}");
        }
        for text in ["HD", "HD00", "HD57", "HDMEM01"] {
            assert_eq!(state.apply_preset(text), None, "{text:?}");
        }
        for text in ["HD", "HDDAB", "HD?"] {
            assert_eq!(state.apply_band_or_mode(text), None, "{text:?}");
        }
        for text in [
            "SIG LEV 7",
            "SIG LEV x",
            "SIG LEV 10",
            "MLT CURRCH 9",
            "MODE",
            "MODE X",
            "ARTISTA",
            "",
        ] {
            assert_eq!(state.hd.apply_status(text), None, "{text:?}");
        }
        assert_eq!(state, TunerState::default());
    }

    #[test]
    fn a_failed_command_leaves_the_text_unchanged() {
        for command in [TunerCommand::Preset(0), TunerCommand::HdMulticast(9)] {
            let mut out = String::from("x");
            assert!(command.write(&mut out).is_err());
            assert_eq!(out, "x");
        }
    }
}
