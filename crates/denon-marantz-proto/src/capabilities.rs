//! Features and value ranges that `Deviceinfo.xml` lists.

use core::fmt;

use crate::error::Error;
use crate::sorted::{self, Keyed};
use crate::sound_settings::SoundSettingValue;
use crate::telnet::{Command, Query, query_for_text};
use crate::volume::{PowerOnVolume, VolumeMode};
use crate::xml::{Child, Node};
use crate::zone::Zone;
use crate::zone_settings::ZoneSettingValue;

/// A range of numbers from `Deviceinfo.xml`, in tenths of the unit of the
/// file: a range from -10 to 0 dB is `-100` to `0`.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValueRange {
    /// Lowest value in tenths.
    pub min_tenths: i16,
    /// Highest value in tenths.
    pub max_tenths: i16,
    /// Step in tenths. `None` if the file has no single step.
    pub step_tenths: Option<u16>,
}

impl ValueRange {
    /// Returns true if the whole number is in the range and is a whole
    /// number of steps above the lowest value.
    #[must_use]
    pub fn contains_whole(self, value: i16) -> bool {
        self.contains(i32::from(value) * 10)
    }

    /// Returns true if the value in tenths is in the range and is a whole
    /// number of steps above the lowest value. A step of 0 is not checked.
    #[must_use]
    pub fn contains_tenths(self, tenths: i16) -> bool {
        self.contains(i32::from(tenths))
    }

    fn contains(self, tenths: i32) -> bool {
        let above_min = tenths - i32::from(self.min_tenths);
        (i32::from(self.min_tenths)..=i32::from(self.max_tenths)).contains(&tenths)
            && self
                .step_tenths
                .filter(|step| *step > 0)
                .is_none_or(|step| above_min % i32::from(step) == 0)
    }

    /// Reads `MinValue`, `MaxValue` and `StepValue` or `Step` of an entry.
    /// `min_tenths` is used when the entry has no `MinValue`.
    fn parse(entry: Node<'_>, min_tenths: Option<i16>) -> Option<Self> {
        let (mut min, mut max, mut step) = (None, None, None);
        for child in entry.children() {
            let slot = if child.is("MinValue") {
                &mut min
            } else if child.is("MaxValue") {
                &mut max
            } else if child.is("StepValue") || child.is("Step") {
                &mut step
            } else {
                continue;
            };
            *slot = slot.or_else(|| parse_tenths(&child.node().text()?));
        }
        let min_tenths = min.or(min_tenths)?;
        let max_tenths = max?;
        let step_tenths = step
            .and_then(|step| u16::try_from(step).ok())
            .filter(|step| *step > 0);
        (min_tenths <= max_tenths).then_some(Self {
            min_tenths,
            max_tenths,
            step_tenths,
        })
    }
}

/// Parses a number with at most one digit after the point, for example
/// `-20.0` or `16`, as tenths.
fn parse_tenths(text: &str) -> Option<i16> {
    let text = text.trim();
    let (negative, digits) = text
        .strip_prefix('-')
        .map_or((false, text), |rest| (true, rest));
    let (whole, tenth) = match digits.split_once('.') {
        Some((whole, "")) => (whole, 0),
        Some((whole, tenth)) if tenth.len() == 1 => (whole, i16::from(tenth.parse::<u8>().ok()?)),
        Some(_) => return None,
        None => (digits, 0),
    };
    if whole.is_empty() || !whole.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value = whole
        .parse::<i16>()
        .ok()?
        .checked_mul(10)?
        .checked_add(tenth)?;
    Some(if negative { -value } else { value })
}

/// Value ranges of one zone from `Deviceinfo.xml`. A value is `None` if the
/// file does not list it.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ZoneRanges {
    /// Volume in dB above -80 dB. The highest value is the end of the scale,
    /// not the volume limit that the user set.
    pub volume: Option<ValueRange>,
    /// Bass in dB. Zone 2 and zone 3 only.
    pub bass: Option<ValueRange>,
    /// Treble in dB. Zone 2 and zone 3 only.
    pub treble: Option<ValueRange>,
    /// Level of the left channel in dB. Zone 2 and zone 3 only.
    pub left_level: Option<ValueRange>,
    /// Level of the right channel in dB. Zone 2 and zone 3 only.
    pub right_level: Option<ValueRange>,
}

impl ZoneRanges {
    const fn list(self) -> [Option<ValueRange>; 5] {
        [
            self.volume,
            self.bass,
            self.treble,
            self.left_level,
            self.right_level,
        ]
    }

    /// The field of an entry of `Zone2Setup` or `Zone3Setup`.
    fn entry_slot(&mut self, name: &str) -> Option<&mut Option<ValueRange>> {
        match name {
            "Bass" => Some(&mut self.bass),
            "Treble" => Some(&mut self.treble),
            "LchLevel" => Some(&mut self.left_level),
            "RchLevel" => Some(&mut self.right_level),
            _ => None,
        }
    }
}

/// Value ranges from `Deviceinfo.xml`. A value is `None` if the file does
/// not list it. Ranges are in the unit of the file: the LFE level is from -10
/// to 0 dB, not dB below 0 dB.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeviceRanges {
    /// Bass sync.
    pub bass_sync: Option<ValueRange>,
    /// Level of a graphic EQ band in dB.
    pub eq_adjust_db: Option<ValueRange>,
    /// Containment amount of Audyssey LFC.
    pub containment_amount: Option<ValueRange>,
    /// Dialog control in dB.
    pub dialog_control: Option<ValueRange>,
    /// LFE level in dB, from a negative number to 0.
    pub lfe: Option<ValueRange>,
    /// LFE level of a 7.1 channel input in dB.
    pub lfe_external_input: Option<ValueRange>,
    /// Auro-Matic strength.
    pub auro_matic_strength: Option<ValueRange>,
    /// Delay time in ms.
    pub delay_time: Option<ValueRange>,
    /// Effect level.
    pub effect_level: Option<ValueRange>,
    /// Audio delay in ms.
    pub audio_delay: Option<ValueRange>,
    /// Ranges of each zone, in the order of [`Zone::ALL`].
    pub zones: [ZoneRanges; 3],
}

impl DeviceRanges {
    /// The ranges of one zone.
    #[must_use]
    pub const fn zone(&self, zone: Zone) -> &ZoneRanges {
        &self.zones[zone.index()]
    }

    /// Each range that the file lists, in no fixed order.
    pub fn all(&self) -> impl Iterator<Item = ValueRange> + use<> {
        [
            self.bass_sync,
            self.eq_adjust_db,
            self.containment_amount,
            self.dialog_control,
            self.lfe,
            self.lfe_external_input,
            self.auro_matic_strength,
            self.delay_time,
            self.effect_level,
            self.audio_delay,
        ]
        .into_iter()
        .chain(self.zones.into_iter().flat_map(ZoneRanges::list))
        .flatten()
    }

    /// Checks a setter against the ranges that the receiver lists. Values of
    /// ranges that it does not list, and other commands, pass.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ValueOutOfRange`] for a value outside a listed range
    /// or between its steps. A volume is checked against the highest volume
    /// of the zone only.
    pub fn check(&self, zone: Zone, command: &Command<'_>) -> Result<(), Error> {
        use SoundSettingValue as S;
        use ZoneSettingValue as Z;
        let zone_ranges = self.zone(zone);
        let volume = match *command {
            Command::SetVolume(volume) => Some((volume, "volume")),
            Command::SetZoneSetting(Z::VolumeLimit(Some(volume))) => Some((volume, "volume limit")),
            Command::SetZoneSetting(Z::PowerOnVolume(PowerOnVolume::Level(volume))) => {
                Some((volume, "power on volume"))
            }
            Command::SetZoneSetting(Z::VolumeMode(VolumeMode::Fixed(volume))) => {
                Some((volume, "fixed volume"))
            }
            _ => None,
        };
        if let Some((volume, what)) = volume {
            // The volume is in steps of 0.5 dB, the range in tenths. Only the
            // highest value is checked: it is the end of the scale.
            let tenths = i32::from(volume.half_db_above_min()) * 5;
            return match zone_ranges.volume {
                Some(range) if tenths > i32::from(range.max_tenths) => {
                    Err(Error::ValueOutOfRange { what })
                }
                _ => Ok(()),
            };
        }
        let (range, value, what) = match *command {
            Command::SetContainmentAmount(amount) => (
                self.containment_amount,
                i32::from(amount.get()),
                "containment amount",
            ),
            Command::SetSoundSetting(setting) => match setting {
                S::AuroMaticStrength(v) => (
                    self.auro_matic_strength,
                    i32::from(v),
                    "Auro-Matic strength",
                ),
                S::BassSync(v) => (self.bass_sync, i32::from(v), "bass sync"),
                S::DialogControl(v) => (self.dialog_control, i32::from(v), "dialog control"),
                // The setter takes dB below 0 dB, the range is negative.
                S::LfeLevel(v) => (self.lfe, -i32::from(v), "LFE level"),
                S::DelayTime(v) => (self.delay_time, i32::from(v), "delay time"),
                S::EffectLevel(v) => (self.effect_level, i32::from(v), "effect level"),
                S::AudioDelay(v) => (self.audio_delay, i32::from(v), "audio delay"),
                S::LfeExternalInput(v) => (
                    self.lfe_external_input,
                    i32::from(v),
                    "LFE level of a 7.1 input",
                ),
                _ => return Ok(()),
            },
            Command::SetZoneSetting(setting) => match setting {
                Z::Bass(level) => (zone_ranges.bass, i32::from(level.db()), "bass"),
                Z::Treble(level) => (zone_ranges.treble, i32::from(level.db()), "treble"),
                Z::LeftLevel(db) => (zone_ranges.left_level, i32::from(db), "left level"),
                Z::RightLevel(db) => (zone_ranges.right_level, i32::from(db), "right level"),
                _ => return Ok(()),
            },
            _ => return Ok(()),
        };
        let inside =
            range.is_none_or(|range| i16::try_from(value).is_ok_and(|v| range.contains_whole(v)));
        if inside {
            Ok(())
        } else {
            Err(Error::ValueOutOfRange { what })
        }
    }

    /// Reads the range of a block of `DeviceCapabilities/Setup` that has one
    /// itself. Returns true if entries inside the block have ranges.
    fn read_block(&mut self, block: &Child<'_>) -> bool {
        match block.local_name() {
            "BassSync" => {
                self.bass_sync = range(block);
                false
            }
            // Files of about 2015 have the range on `AudioDelay` itself.
            // Newer files have it in `Adjust`.
            "AudioDelay" => {
                self.audio_delay = range(block);
                true
            }
            "Zone2Setup" | "Zone3Setup" | "SurroundParameter" | "GraphicEQ" | "Audyssey" => true,
            _ => false,
        }
    }

    /// Reads the range of an entry inside the block with the name `block`.
    fn read_entry(&mut self, block: &str, entry: &Child<'_>) {
        let slot = match (block, entry.local_name()) {
            ("Zone2Setup", name) => self.zones[1].entry_slot(name),
            ("Zone3Setup", name) => self.zones[2].entry_slot(name),
            ("SurroundParameter", "DialogControl") => Some(&mut self.dialog_control),
            ("SurroundParameter", "AuroMatic3DStrength") => Some(&mut self.auro_matic_strength),
            ("SurroundParameter", "DelayTime") => Some(&mut self.delay_time),
            ("SurroundParameter", "EffectLevel") => Some(&mut self.effect_level),
            ("GraphicEQ", "EQAdjustDB") => Some(&mut self.eq_adjust_db),
            ("Audyssey", "ContainmentAmount") => Some(&mut self.containment_amount),
            ("SurroundParameter", "LFE") => {
                for part in entry.node().children() {
                    if part.is("Normal") {
                        self.lfe = range(&part);
                    } else if part.is("ExtIn") {
                        self.lfe_external_input = range(&part);
                    }
                }
                None
            }
            ("AudioDelay", "Adjust") => {
                if let Some(adjust) = range(entry) {
                    self.audio_delay = Some(adjust);
                }
                None
            }
            _ => None,
        };
        if let Some(slot) = slot {
            *slot = range(entry);
        }
    }

    /// Reads the volume range of each zone. `Zone/No` tells the zone: 0 is
    /// the main zone.
    fn read_zones(&mut self, root: Node<'_>) {
        for block in root.children().filter(|c| c.is("DeviceZoneCapabilities")) {
            let block = block.node();
            let number = block
                .child("Zone")
                .and_then(|zone| zone.node().child("No"))
                .and_then(|number| number.node().text()?.trim().parse::<usize>().ok());
            let Some(zone) = number.and_then(|number| self.zones.get_mut(number)) else {
                continue;
            };
            // The volume has no `MinValue`: it starts at 0 (-80 dB).
            zone.volume = block
                .child("Volume")
                .and_then(|volume| ValueRange::parse(volume.node(), Some(0)));
        }
    }
}

/// Reads a range from `MinValue`, `MaxValue` and `StepValue` or `Step`.
fn range(child: &Child<'_>) -> Option<ValueRange> {
    ValueRange::parse(child.node(), None)
}

/// Reads the features and the ranges of `Deviceinfo.xml` in one walk over
/// `DeviceCapabilities/Setup`. The features are `None` if the file does not
/// list `SurroundParameter/LFE`: files of about 2015 and older list only a
/// few features, and their receivers answer more queries.
pub(crate) fn parse(root: Node<'_>) -> (Option<DeviceFeatures>, DeviceRanges) {
    let mut ranges = DeviceRanges::default();
    ranges.read_zones(root);
    let Some(setup) = root
        .child("DeviceCapabilities")
        .and_then(|c| c.node().child("Setup"))
    else {
        return (None, ranges);
    };
    let mut features = DeviceFeatures::default();
    for block in setup.node().children() {
        let name = block.local_name();
        let ranges_inside = ranges.read_block(&block);
        let features_inside = features.read_block(name);
        if !ranges_inside && !features_inside {
            continue;
        }
        for entry in block.node().children() {
            if ranges_inside {
                ranges.read_entry(name, &entry);
            }
            if features_inside {
                features.read_entry(name, &entry);
            }
        }
    }
    let listed = features.has(DeviceFeature::Lfe);
    (listed.then_some(features), ranges)
}

/// Defines [`DeviceFeature`], its tag paths and the list of all values.
macro_rules! device_features {
    ($($(#[$doc:meta])* $name:ident = $path:expr,)*) => {
        /// An entry of `Deviceinfo.xml` that tells that the receiver has a
        /// feature.
        #[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum DeviceFeature {
            $($(#[$doc])* $name,)*
        }

        impl DeviceFeature {
            /// All values.
            pub const ALL: &'static [Self] = &[$(Self::$name,)*];

            /// Where the entry is below `DeviceCapabilities/Setup`.
            const fn path(self) -> Path {
                match self {
                    $(Self::$name => $path,)*
                }
            }
        }
    };
}

/// Where an entry is.
#[derive(Debug, Clone, Copy)]
enum Path {
    /// A block of `DeviceCapabilities/Setup`.
    Block(&'static str),
    /// An entry inside a block of `DeviceCapabilities/Setup`.
    In(&'static str, &'static str),
    /// A block of `DeviceCapabilities/Setup`, or an entry inside one of
    /// [`OUTER_BLOCKS`].
    Anywhere(&'static str),
}

use Path::{Anywhere, Block, In};

/// The blocks that can hold an entry of [`Path::Anywhere`].
const OUTER_BLOCKS: [&str; 7] = [
    "SurroundParameter",
    "OutputSettings",
    "HdmiSetup",
    "Audyssey",
    "AudioDelay",
    "InputSelect",
    "FrontDisplay",
];

device_features! {
    /// `SurroundParameter/SubwooferMode`: IMAX subwoofer mode.
    SubwooferMode = In("SurroundParameter", "SubwooferMode"),
    /// `SurroundParameter/IMAX`.
    Imax = In("SurroundParameter", "IMAX"),
    /// `SurroundParameter/IMAXAudioSettings`.
    ImaxAudioSettings = In("SurroundParameter", "IMAXAudioSettings"),
    /// `SurroundParameter/HighPassFilter`: IMAX high pass filter.
    HighPassFilter = In("SurroundParameter", "HighPassFilter"),
    /// `SurroundParameter/LowPassFilter`: IMAX low pass filter.
    LowPassFilter = In("SurroundParameter", "LowPassFilter"),
    /// `SurroundParameter/SubwooferOutput`: IMAX subwoofer output.
    SubwooferOutput = In("SurroundParameter", "SubwooferOutput"),
    /// `SurroundParameter/AuroMatic3DPreset`.
    AuroMatic3dPreset = In("SurroundParameter", "AuroMatic3DPreset"),
    /// `SurroundParameter/AuroMatic3DStrength`.
    AuroMatic3dStrength = In("SurroundParameter", "AuroMatic3DStrength"),
    /// `SurroundParameter/Auro3DMode`.
    Auro3dMode = In("SurroundParameter", "Auro3DMode"),
    /// `SurroundParameter/RoomSize`.
    RoomSize = In("SurroundParameter", "RoomSize"),
    /// `SurroundParameter/SpeakerVirtualizer`.
    SpeakerVirtualizer = In("SurroundParameter", "SpeakerVirtualizer"),
    /// `SurroundParameter/CenterSpread`.
    CenterSpread = In("SurroundParameter", "CenterSpread"),
    /// `SurroundParameter/DTSNeuralX`.
    DtsNeuralX = In("SurroundParameter", "DTSNeuralX"),
    /// `SurroundParameter/Loudness`: loudness management.
    Loudness = In("SurroundParameter", "Loudness"),
    /// `SurroundParameter/CinemaEq`.
    CinemaEq = In("SurroundParameter", "CinemaEq"),
    /// `SurroundParameter/DynamicCompression`: DRC.
    DynamicCompression = In("SurroundParameter", "DynamicCompression"),
    /// `SurroundParameter/DialogControl`.
    DialogControl = In("SurroundParameter", "DialogControl"),
    /// `SurroundParameter/LFE`.
    Lfe = In("SurroundParameter", "LFE"),
    /// `SurroundParameter/DelayTime`.
    DelayTime = In("SurroundParameter", "DelayTime"),
    /// `SurroundParameter/EffectLevel`.
    EffectLevel = In("SurroundParameter", "EffectLevel"),
    /// `SurroundParameter/SpeakerSelect`: effect speaker selection.
    SpeakerSelect = In("SurroundParameter", "SpeakerSelect"),
    /// `SurroundParameter/Subwoofer`.
    Subwoofer = In("SurroundParameter", "Subwoofer"),
    /// `SurroundLevelCompensation`, as a block or inside one.
    SurroundLevelCompensation = Anywhere("SurroundLevelCompensation"),
    /// `DialogEnhancer`.
    DialogEnhancer = Block("DialogEnhancer"),
    /// `BassSync`.
    BassSync = Block("BassSync"),
    /// `BTTX`: Bluetooth transmitter.
    Bttx = Block("BTTX"),
    /// `SpeakerPreset`.
    SpeakerPreset = Block("SpeakerPreset"),
    /// `DiracLive`.
    DiracLive = Block("DiracLive"),
    /// `ToneControl`.
    ToneControl = Block("ToneControl"),
    /// `AllZoneStereo`.
    AllZoneStereo = Block("AllZoneStereo"),
    /// `PictureMode`.
    PictureMode = Block("PictureMode"),
    /// `OutputSettings/AspectRatio`.
    AspectRatio = In("OutputSettings", "AspectRatio"),
    /// `OutputSettings/ResolutionAnalog`.
    ResolutionAnalog = In("OutputSettings", "ResolutionAnalog"),
    /// `OutputSettings/HDMIUpscaler`.
    HdmiUpscaler = In("OutputSettings", "HDMIUpscaler"),
    /// `OutputSettings/VideoMode`.
    VideoMode = In("OutputSettings", "VideoMode"),
    /// `OutputSettings/HdmiVideoOut`.
    HdmiVideoOut = In("OutputSettings", "HdmiVideoOut"),
    /// `HdmiSetup/HdmiAudioOut`.
    HdmiAudioOut = In("HdmiSetup", "HdmiAudioOut"),
    /// `Audyssey/DynamicEq`.
    DynamicEq = In("Audyssey", "DynamicEq"),
    /// `Audyssey/MultEq`.
    MultEq = In("Audyssey", "MultEq"),
    /// `Audyssey/RefLevOffset`.
    ReferenceLevelOffset = In("Audyssey", "RefLevOffset"),
    /// `Audyssey/DynamicVolume`.
    DynamicVolume = In("Audyssey", "DynamicVolume"),
    /// `Audyssey/AudysseyLfc`.
    AudysseyLfc = In("Audyssey", "AudysseyLfc"),
    /// `Audyssey/ContainmentAmount`.
    ContainmentAmount = In("Audyssey", "ContainmentAmount"),
    /// `ECO`.
    Eco = Block("ECO"),
    /// `VideoSelect`.
    VideoSelect = Block("VideoSelect"),
    /// `AudioDelay`.
    AudioDelay = Block("AudioDelay"),
    /// `AudioDelay/AutoLipSync`.
    AutoLipSync = In("AudioDelay", "AutoLipSync"),
    /// `InputSelect/InputMode`.
    InputMode = In("InputSelect", "InputMode"),
    /// `SpeakerAB`: front speakers A and B.
    SpeakerAb = Block("SpeakerAB"),
    /// `Restorer`.
    Restorer = Block("Restorer"),
    /// `DialogLevel`.
    DialogLevel = Block("DialogLevel"),
    /// `FrontDisplay/Dimmer`.
    Dimmer = In("FrontDisplay", "Dimmer"),
    /// `Zone3Setup`: the receiver has zone 3.
    Zone3Setup = Block("Zone3Setup"),
    /// `SoundMode/Genre`: the sound mode categories movie, music, game and
    /// pure.
    SoundModeGenre = In("SoundMode", "Genre"),
}

impl DeviceFeature {
    /// The bit of the feature. Stored values hold these bits, so new
    /// features go at the end of the list.
    const fn bit(self) -> u64 {
        1 << self as u8
    }
}

const _: () = assert!(DeviceFeature::ALL.len() <= 64);

/// The features by block name and entry name. The entry name is empty for a
/// block.
static BY_PATH: [Keyed<DeviceFeature>; PATH_COUNT] = sorted::sort(paths());

/// The number of keys in [`BY_PATH`].
const PATH_COUNT: usize = {
    let mut count = 0;
    let mut i = 0;
    while i < DeviceFeature::ALL.len() {
        count += match DeviceFeature::ALL[i].path() {
            Block(_) | In(..) => 1,
            Anywhere(_) => 1 + OUTER_BLOCKS.len(),
        };
        i += 1;
    }
    count
};

const fn paths() -> [Keyed<DeviceFeature>; PATH_COUNT] {
    const fn keyed(
        block: &'static str,
        entry: &'static str,
        value: DeviceFeature,
    ) -> Keyed<DeviceFeature> {
        Keyed {
            key: (block, entry),
            value,
        }
    }
    let mut list = [keyed("", "", DeviceFeature::Imax); PATH_COUNT];
    let mut count = 0;
    let mut i = 0;
    while i < DeviceFeature::ALL.len() {
        let feature = DeviceFeature::ALL[i];
        match feature.path() {
            Block(name) => {
                list[count] = keyed(name, "", feature);
                count += 1;
            }
            In(block, name) => {
                list[count] = keyed(block, name, feature);
                count += 1;
            }
            Anywhere(name) => {
                list[count] = keyed(name, "", feature);
                count += 1;
                let mut j = 0;
                while j < OUTER_BLOCKS.len() {
                    list[count] = keyed(OUTER_BLOCKS[j], name, feature);
                    count += 1;
                    j += 1;
                }
            }
        }
        i += 1;
    }
    list
}

/// The features that `Deviceinfo.xml` lists.
///
/// The library does not send the state queries of a feature that the file
/// does not list. The choice lists in the file are not read: they can lack
/// values that the receiver accepts.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub struct DeviceFeatures(u64);

impl DeviceFeatures {
    /// Returns true if the file lists the feature.
    #[must_use]
    pub const fn has(self, feature: DeviceFeature) -> bool {
        self.0 & feature.bit() != 0
    }

    /// Returns these features with `feature` added.
    #[must_use]
    pub const fn with(self, feature: DeviceFeature) -> Self {
        Self(self.0 | feature.bit())
    }

    /// Returns false if the query asks for a feature that the file does not
    /// list. True for a query that needs no feature, and for a query that
    /// this library does not know.
    #[must_use]
    pub fn allows(self, query: &str) -> bool {
        query_for_text(query).is_none_or(|query| query.listed_in(Some(self)))
    }

    /// Adds the feature of a block of `DeviceCapabilities/Setup`. Returns
    /// true if entries inside the block are features.
    fn read_block(&mut self, name: &str) -> bool {
        if let Some(feature) = sorted::find(&BY_PATH, (name, "")) {
            *self = self.with(feature);
        }
        sorted::has_second(&BY_PATH, name)
    }

    /// Adds the feature of an entry inside the block with the name `block`.
    fn read_entry(&mut self, block: &str, entry: &Child<'_>) {
        if let Some(feature) = sorted::find(&BY_PATH, (block, entry.local_name())) {
            *self = self.with(feature);
        }
    }

    /// The features that the file lists, in the order of
    /// [`DeviceFeature::ALL`].
    pub fn iter(self) -> impl Iterator<Item = DeviceFeature> {
        DeviceFeature::ALL
            .iter()
            .copied()
            .filter(move |feature| self.has(*feature))
    }
}

impl fmt::Debug for DeviceFeatures {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl FromIterator<DeviceFeature> for DeviceFeatures {
    fn from_iter<I: IntoIterator<Item = DeviceFeature>>(features: I) -> Self {
        features.into_iter().fold(Self::default(), Self::with)
    }
}

impl Query {
    /// The feature that the receiver must list in `Deviceinfo.xml` to be
    /// asked for this value at the start, with a refresh and with a group
    /// refresh. `None` if it is asked on every receiver. `CV?` and the quick
    /// select names need no feature: the file does not tell if the receiver
    /// answers them.
    #[must_use]
    pub const fn device_feature(self) -> Option<DeviceFeature> {
        use DeviceFeature as F;
        if matches!(self.zone(), Zone::Zone3) {
            return Some(F::Zone3Setup);
        }
        Some(match self {
            Self::ImaxSubwooferMode => F::SubwooferMode,
            Self::SoundModeCategory => F::SoundModeGenre,
            Self::Imax => F::Imax,
            Self::ImaxAudio => F::ImaxAudioSettings,
            Self::ImaxHighPassFilter => F::HighPassFilter,
            Self::ImaxLowPassFilter => F::LowPassFilter,
            Self::ImaxSubwooferOutput => F::SubwooferOutput,
            Self::AuroPreset => F::AuroMatic3dPreset,
            Self::AuroStrength => F::AuroMatic3dStrength,
            Self::AuroMode => F::Auro3dMode,
            Self::RoomSize => F::RoomSize,
            Self::SpeakerVirtualizer => F::SpeakerVirtualizer,
            Self::CenterSpread => F::CenterSpread,
            Self::NeuralX => F::DtsNeuralX,
            Self::LoudnessManagement => F::Loudness,
            Self::CinemaEq => F::CinemaEq,
            Self::Drc => F::DynamicCompression,
            Self::DialogControl => F::DialogControl,
            Self::LfeLevel => F::Lfe,
            Self::DelayTime => F::DelayTime,
            Self::Effect => F::EffectLevel,
            Self::EffectSpeakers => F::SpeakerSelect,
            Self::Subwoofer => F::Subwoofer,
            Self::SurroundLevel => F::SurroundLevelCompensation,
            Self::DialogEnhancer => F::DialogEnhancer,
            Self::BassSync => F::BassSync,
            Self::BluetoothTransmitter | Self::BluetoothLevel => F::Bttx,
            Self::SpeakerPreset => F::SpeakerPreset,
            Self::DiracSlots | Self::DiracFilter => F::DiracLive,
            Self::ToneControl | Self::Bass | Self::Treble => F::ToneControl,
            Self::AllZoneStereo => F::AllZoneStereo,
            Self::PictureMode
            | Self::Contrast
            | Self::Brightness
            | Self::Saturation
            | Self::NoiseReduction
            | Self::Enhancer => F::PictureMode,
            Self::AspectRatio => F::AspectRatio,
            Self::AnalogToHdmiResolution => F::ResolutionAnalog,
            Self::HdmiUpscaler => F::HdmiUpscaler,
            Self::VideoProcess => F::VideoMode,
            Self::HdmiOutput => F::HdmiVideoOut,
            Self::HdmiAudioDecode => F::HdmiAudioOut,
            Self::DynamicEq => F::DynamicEq,
            Self::MultEq => F::MultEq,
            Self::ReferenceLevel => F::ReferenceLevelOffset,
            Self::DynamicVolume => F::DynamicVolume,
            Self::Lfc => F::AudysseyLfc,
            Self::ContainmentAmount => F::ContainmentAmount,
            Self::Eco => F::Eco,
            Self::VideoSelect => F::VideoSelect,
            Self::AudioDelay => F::AudioDelay,
            Self::AutoLipSync | Self::AutoLipSyncAls => F::AutoLipSync,
            Self::AudioInputMode => F::InputMode,
            Self::FrontSpeaker => F::SpeakerAb,
            Self::AudioRestorer => F::Restorer,
            Self::DialogLevel => F::DialogLevel,
            Self::Dimmer => F::Dimmer,
            _ => return None,
        })
    }

    /// Returns true if `features` has the [`Query::device_feature`] of the
    /// value. True if the value needs none, or if `features` is `None`. The
    /// tuner queries need none: newer receivers with a tuner do not list
    /// `TunerOperation`.
    #[must_use]
    pub const fn listed_in(self, features: Option<DeviceFeatures>) -> bool {
        match (self.device_feature(), features) {
            (Some(feature), Some(features)) => features.has(feature),
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::volume::Volume;

    #[test]
    fn tenths_parse() {
        assert_eq!(parse_tenths("-20.0"), Some(-200));
        assert_eq!(parse_tenths(" 16 "), Some(160));
        assert_eq!(parse_tenths("0.5"), Some(5));
        assert_eq!(parse_tenths("98."), Some(980));
        assert_eq!(parse_tenths("-0.5"), Some(-5));
        for bad in ["", "-", ".5", "1.25", "x", "1e3", "+1", "4000", "--1"] {
            assert_eq!(parse_tenths(bad), None, "{bad}");
        }
    }

    #[test]
    fn range_contains_whole_numbers() {
        let range = ValueRange {
            min_tenths: -100,
            max_tenths: 0,
            step_tenths: Some(10),
        };
        assert!(range.contains_whole(-10));
        assert!(range.contains_whole(0));
        assert!(!range.contains_whole(1));
        assert!(!range.contains_whole(-11));
        assert!(!range.contains_whole(i16::MAX));
        assert!(!range.contains_whole(i16::MIN));
        let even = ValueRange {
            min_tenths: -140,
            max_tenths: 140,
            step_tenths: Some(20),
        };
        assert!(even.contains_whole(-14));
        assert!(even.contains_whole(0));
        assert!(!even.contains_whole(13));
        assert!(!even.contains_whole(-13));
        let half = ValueRange {
            min_tenths: -200,
            max_tenths: 60,
            step_tenths: Some(5),
        };
        assert!(half.contains_tenths(-195));
        assert!(half.contains_tenths(60));
        assert!(!half.contains_tenths(-197));
        assert!(!half.contains_tenths(65));
        let no_step = ValueRange {
            step_tenths: Some(0),
            ..half
        };
        assert!(no_step.contains_tenths(-197));
        assert!(no_step.contains_whole(-20));
    }

    #[test]
    fn check_uses_the_listed_ranges() {
        let mut ranges = DeviceRanges::default();
        ranges.zones[1].volume = Some(ValueRange {
            min_tenths: 0,
            max_tenths: 600,
            step_tenths: Some(10),
        });
        let volume = |db| Command::SetVolume(Volume::from_db_rounded(db).unwrap());
        assert!(ranges.check(Zone::Zone2, &volume(-20.0)).is_ok());
        assert!(ranges.check(Zone::Zone2, &volume(-19.5)).is_err());
        assert!(ranges.check(Zone::Main, &volume(18.0)).is_ok());
        let zone2 = |setting| ranges.check(Zone::Zone2, &Command::SetZoneSetting(setting));
        let db = |db| Volume::from_db_rounded(db).unwrap();
        assert!(zone2(ZoneSettingValue::VolumeLimit(Some(db(-20.0)))).is_ok());
        assert!(zone2(ZoneSettingValue::VolumeLimit(Some(db(-10.0)))).is_err());
        assert!(
            zone2(ZoneSettingValue::PowerOnVolume(PowerOnVolume::Level(db(
                -19.0
            ))))
            .is_err()
        );
        assert!(zone2(ZoneSettingValue::VolumeMode(VolumeMode::Fixed(db(-19.0)))).is_err());
        assert!(zone2(ZoneSettingValue::VolumeLimit(None)).is_ok());
        let sound = |value| Command::SetSoundSetting(SoundSettingValue::DelayTime(value));
        assert!(ranges.check(Zone::Main, &sound(300)).is_ok());
        ranges.delay_time = Some(ValueRange {
            min_tenths: 0,
            max_tenths: 2000,
            step_tenths: None,
        });
        assert!(ranges.check(Zone::Main, &sound(200)).is_ok());
        assert_eq!(
            ranges.check(Zone::Main, &sound(201)),
            Err(Error::ValueOutOfRange { what: "delay time" })
        );
        assert!(ranges.check(Zone::Main, &Command::Raw("PSDEL 999")).is_ok());
        let lfe_input =
            |value| Command::SetSoundSetting(SoundSettingValue::LfeExternalInput(value));
        assert!(ranges.check(Zone::Main, &lfe_input(15)).is_ok());
        ranges.lfe_external_input = Some(ValueRange {
            min_tenths: 0,
            max_tenths: 100,
            step_tenths: Some(50),
        });
        assert!(ranges.check(Zone::Main, &lfe_input(10)).is_ok());
        assert_eq!(
            ranges.check(Zone::Main, &lfe_input(15)),
            Err(Error::ValueOutOfRange {
                what: "LFE level of a 7.1 input"
            })
        );
    }

    #[test]
    fn both_auto_lip_sync_queries_need_the_feature() {
        let none = DeviceFeatures::default();
        let listed = none.with(DeviceFeature::AutoLipSync);
        for query in ["OPALS ?", "SSHOS ?", "SSALS ?"] {
            assert!(!none.allows(query), "{query}");
            assert!(listed.allows(query), "{query}");
        }
    }

    #[test]
    fn zone_volume_ranges_follow_the_zone_number() {
        let zone = |number: &str, max: &str| {
            format!(
                "<DeviceZoneCapabilities><Zone><No>{number}</No></Zone>\
                 <Volume><MaxValue>{max}</MaxValue></Volume></DeviceZoneCapabilities>"
            )
        };
        let content = [
            zone("2", "60"),
            zone("0", "98"),
            zone("7", "10"),
            zone("x", "10"),
        ]
        .concat();
        let (_, ranges) = parse(Node::new(content.as_bytes()));
        let max = |zone: Zone| ranges.zone(zone).volume.map(|range| range.max_tenths);
        assert_eq!(max(Zone::Main), Some(980));
        assert_eq!(max(Zone::Zone2), None);
        assert_eq!(max(Zone::Zone3), Some(600));
    }

    #[test]
    fn every_feature_has_its_own_bit() {
        let all: DeviceFeatures = DeviceFeature::ALL.iter().copied().collect();
        assert_eq!(all.0.count_ones() as usize, DeviceFeature::ALL.len());
        assert!(all.iter().eq(DeviceFeature::ALL.iter().copied()));
        let two = DeviceFeatures::default()
            .with(DeviceFeature::Imax)
            .with(DeviceFeature::Dimmer);
        assert_eq!(format!("{two:?}"), "{Imax, Dimmer}");
    }

    #[test]
    fn every_feature_path_is_found() {
        // Each path is listed once.
        assert!(sorted::is_strictly_sorted(&BY_PATH));
        for feature in DeviceFeature::ALL {
            let found = |key| sorted::find(&BY_PATH, key) == Some(*feature);
            match feature.path() {
                Block(name) => assert!(found((name, ""))),
                In(block, name) => {
                    assert!(found((block, name)));
                    assert!(sorted::has_second(&BY_PATH, block));
                }
                Anywhere(name) => {
                    assert!(found((name, "")));
                    assert!(OUTER_BLOCKS.iter().all(|block| found((block, name))));
                }
            }
        }
        assert!(!sorted::has_second(&BY_PATH, "BassSync"));
    }
}
