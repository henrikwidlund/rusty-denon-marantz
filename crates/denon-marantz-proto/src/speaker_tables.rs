//! Speaker distances and crossovers from the web control pages of a receiver.
//!
//! The pages are `/ajax/speakers/get_config?type=N` on port 11080. They list
//! speakers by an index. Each page has its own order of speakers. The orders
//! come from the script of the web control of an AVC-A10H. Entries with an
//! index that is not known are skipped.

use crate::channel::Channel;
use crate::error::Error;
use crate::speaker_setup::SpeakerGroup;
use crate::xml::{Node, check_well_formed};

/// Port of the web control pages.
pub const AJAX_PORT: u16 = 11080;
/// Path of the speaker distances page.
pub const DISTANCES_PATH: &str = "/ajax/speakers/get_config?type=4";
/// Path of the speaker crossovers page.
pub const CROSSOVERS_PATH: &str = "/ajax/speakers/get_config?type=6";

/// Path of the bass page.
pub const BASS_PATH: &str = "/ajax/speakers/get_config?type=7";
/// Path of the channel level page, the dials of the web control. The levels
/// are a temporary change on top of the speaker levels.
pub const CHANNEL_LEVELS_PATH: &str = "/ajax/option/get_config?type=3";

/// The subwoofer setup code is this number plus the number of subwoofers.
const SUBWOOFER_CODE_BASE: u8 = 5;

/// Most entries of a table that are kept.
const MAX_ENTRIES: usize = 64;

/// Speakers of the distances page, in the order of the index.
const DISTANCE_ORDER: [Channel; 34] = [
    Channel::FrontLeft,
    Channel::FrontRight,
    Channel::Center,
    Channel::Subwoofer,
    Channel::Subwoofer2,
    Channel::Subwoofer3,
    Channel::Subwoofer4,
    Channel::SurroundLeft,
    Channel::SurroundRight,
    Channel::SurroundBackLeft,
    Channel::SurroundBackRight,
    Channel::SurroundBack,
    Channel::FrontWideLeft,
    Channel::FrontWideRight,
    Channel::FrontHeightLeft,
    Channel::FrontHeightRight,
    Channel::FrontDolbyLeft,
    Channel::FrontDolbyRight,
    Channel::TopFrontLeft,
    Channel::TopFrontRight,
    Channel::TopMiddleLeft,
    Channel::TopMiddleRight,
    Channel::SurroundDolbyLeft,
    Channel::SurroundDolbyRight,
    Channel::TopRearLeft,
    Channel::TopRearRight,
    Channel::SurroundHeightLeft,
    Channel::SurroundHeightRight,
    Channel::RearHeightLeft,
    Channel::RearHeightRight,
    Channel::BackDolbyLeft,
    Channel::BackDolbyRight,
    Channel::CenterHeight,
    Channel::TopSurround,
];

/// Channels of the channel level page, in the order of the index.
const CHANNEL_LEVEL_ORDER: [Channel; 34] = [
    Channel::Center,
    Channel::Subwoofer,
    Channel::Subwoofer2,
    Channel::FrontLeft,
    Channel::FrontRight,
    Channel::SurroundLeft,
    Channel::SurroundRight,
    Channel::SurroundBackLeft,
    Channel::SurroundBackRight,
    Channel::SurroundBack,
    Channel::FrontWideLeft,
    Channel::FrontWideRight,
    Channel::FrontHeightLeft,
    Channel::FrontHeightRight,
    Channel::TopFrontLeft,
    Channel::TopFrontRight,
    Channel::TopMiddleLeft,
    Channel::TopMiddleRight,
    Channel::TopRearLeft,
    Channel::TopRearRight,
    Channel::RearHeightLeft,
    Channel::RearHeightRight,
    Channel::SurroundHeightLeft,
    Channel::SurroundHeightRight,
    Channel::FrontDolbyLeft,
    Channel::FrontDolbyRight,
    Channel::SurroundDolbyLeft,
    Channel::SurroundDolbyRight,
    Channel::BackDolbyLeft,
    Channel::BackDolbyRight,
    Channel::CenterHeight,
    Channel::TopSurround,
    Channel::Subwoofer3,
    Channel::Subwoofer4,
];

/// Groups of the crossovers page, in the order of the index.
const CROSSOVER_ORDER: [SpeakerGroup; 16] = [
    SpeakerGroup::Front,
    SpeakerGroup::Center,
    SpeakerGroup::Surround,
    SpeakerGroup::SurroundBack,
    SpeakerGroup::FrontWide,
    SpeakerGroup::FrontHeight,
    SpeakerGroup::FrontDolby,
    SpeakerGroup::TopFront,
    SpeakerGroup::TopMiddle,
    SpeakerGroup::SurroundDolby,
    SpeakerGroup::TopRear,
    SpeakerGroup::SurroundHeight,
    SpeakerGroup::RearHeight,
    SpeakerGroup::BackDolby,
    SpeakerGroup::CenterHeight,
    SpeakerGroup::TopSurround,
];

/// One value of a speaker or a group of speakers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableEntry<K> {
    /// The speaker or the group.
    pub key: K,
    /// Value as the page gives it.
    pub value: i32,
    /// The `display` attribute of the page. The web control shows an entry
    /// with `2` or `3` and hides one with `1`. With `3` the value can be
    /// changed, with `2` it is shown but cannot.
    pub display: Option<u8>,
}

impl<K> TableEntry<K> {
    /// Returns true if the web control shows the entry.
    #[must_use]
    pub const fn is_shown(&self) -> bool {
        matches!(self.display, Some(2 | 3))
    }

    /// Returns true if the web control lets the user change the value.
    #[must_use]
    pub const fn is_editable(&self) -> bool {
        matches!(self.display, Some(3))
    }
}

/// Unit of the values of the distances page. The page says which one it
/// uses. The unit follows the setting of the receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DistanceUnit {
    /// Meters. A value is centimeters: `250` is 2.50 m.
    Meters,
    /// Feet. A value is centimeters: `305` is 10.0 ft.
    Feet,
    /// Milliseconds. A value is tenths of a millisecond: `25` is 2.5 ms.
    Milliseconds,
}

impl DistanceUnit {
    /// Centimeters in a foot.
    const CM_PER_FOOT: f32 = 30.48;

    /// The unit for the number of the `Unit` element: `1`, `2` or `3`.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Meters),
            2 => Some(Self::Feet),
            3 => Some(Self::Milliseconds),
            _ => None,
        }
    }

    /// Name of the unit, for example `ms`.
    #[must_use]
    pub const fn symbol(self) -> &'static str {
        match self {
            Self::Meters => "m",
            Self::Feet => "ft",
            Self::Milliseconds => "ms",
        }
    }

    /// A value of the page in this unit, for example `25` is `2.5` for
    /// milliseconds.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // Values are small.
    pub fn from_raw(self, raw: i32) -> f32 {
        let raw = raw as f32;
        match self {
            Self::Meters => raw / 100.0,
            Self::Feet => raw / Self::CM_PER_FOOT,
            Self::Milliseconds => raw / 10.0,
        }
    }
}

/// A table of the speaker pages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpeakerTable<K> {
    /// Entries in the order of the page.
    pub entries: Vec<TableEntry<K>>,
    /// Values that the page offers for the setting, for example crossover
    /// frequencies. Empty if the page has no list.
    pub choices: Vec<i32>,
    /// The `Unit` of the page. Only the distances page has it. See
    /// [`SpeakerTable::distance_unit`].
    pub unit: Option<u8>,
    /// Number of subwoofers, from 1 to 4. The distances page has it. The
    /// receiver sends a setup code that is 5 plus the number.
    pub subwoofer_count: Option<u8>,
}

impl<K> SpeakerTable<K> {
    /// The unit of the distances: meters, feet or milliseconds. `None` if the
    /// page has no unit or a number that is not known.
    #[must_use]
    pub fn distance_unit(&self) -> Option<DistanceUnit> {
        self.unit.and_then(DistanceUnit::from_code)
    }
}

impl SpeakerTable<Channel> {
    /// Reads the distances page.
    ///
    /// The unit of the values is in [`Self::unit`].
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML or is an HTML page.
    pub fn parse_distances(body: &[u8]) -> Result<Self, Error> {
        parse(body, |index| DISTANCE_ORDER.get(index).copied())
    }
}

impl SpeakerTable<Channel> {
    /// Reads the channel level page. The values are tenths of a dB: `-20` is
    /// -2 dB. The receiver lists all 34 channels. The channels of the current
    /// speaker setup are the shown ones, see [`TableEntry::is_shown`].
    ///
    /// The receiver answers this page with HTTP status 500 when the dials
    /// cannot be used, for example while a Dirac ART filter is active.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML or is an HTML page.
    pub fn parse_channel_levels(body: &[u8]) -> Result<Self, Error> {
        parse(body, |index| CHANNEL_LEVEL_ORDER.get(index).copied())
    }
}

impl SpeakerTable<SpeakerGroup> {
    /// Reads the crossovers page. The values are in Hz, `0` is full range.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML or is an HTML page.
    pub fn parse_crossovers(body: &[u8]) -> Result<Self, Error> {
        parse(body, |index| CROSSOVER_ORDER.get(index).copied())
    }
}

/// Checks the body and returns the root element.
pub(crate) fn root(body: &[u8]) -> Result<crate::xml::Child<'_>, Error> {
    check_well_formed(body)?;
    let root = Node::new(body)
        .children()
        .next()
        .ok_or(Error::HtmlResponse)?;
    if root.is("html") {
        return Err(Error::HtmlResponse);
    }
    Ok(root)
}

/// Number in the text of the child `name`.
pub(crate) fn number<T: std::str::FromStr>(node: Node<'_>, name: &str) -> Option<T> {
    node.child(name)
        .and_then(|child| child.node().text())
        .and_then(|text| text.trim().parse().ok())
}

/// Reads the `Speaker` elements of a `List`. Entries that are not known or
/// have no number are skipped.
fn read_entries<K>(list: Node<'_>, key: impl Fn(usize) -> Option<K>) -> Vec<TableEntry<K>> {
    let mut entries = Vec::new();
    for speaker in list.children().filter(|c| c.is("Speaker")) {
        let index = speaker
            .attribute("index")
            .and_then(|index| index.trim().parse::<usize>().ok());
        let value = speaker
            .node()
            .text()
            .and_then(|text| text.trim().parse().ok());
        if let (Some(key), Some(value)) = (index.and_then(&key), value) {
            if entries.len() == MAX_ENTRIES {
                break;
            }
            entries.push(TableEntry {
                key,
                value,
                display: speaker
                    .attribute("display")
                    .and_then(|display| display.trim().parse().ok()),
            });
        }
    }
    entries
}

/// Reads the `Item` elements of a `List`.
fn read_choices(list: Node<'_>) -> Vec<i32> {
    list.children()
        .filter(|c| c.is("Item"))
        .filter_map(|item| item.node().text()?.trim().parse().ok())
        .take(MAX_ENTRIES)
        .collect()
}

fn parse<K>(body: &[u8], key: impl Fn(usize) -> Option<K>) -> Result<SpeakerTable<K>, Error> {
    let root = root(body)?;
    let node = root.node();
    Ok(SpeakerTable {
        entries: node
            .child("List")
            .map(|list| read_entries(list.node(), key))
            .unwrap_or_default(),
        choices: node
            .child("SelectableValue")
            .and_then(|selectable| selectable.node().child("List"))
            .map(|list| read_choices(list.node()))
            .unwrap_or_default(),
        unit: number::<i32>(node, "Unit").and_then(|unit| u8::try_from(unit).ok()),
        subwoofer_count: node
            .child("SubwooferType")
            .and_then(|kind| number::<u8>(kind.node(), "CurrentNumber"))
            .and_then(|code| code.checked_sub(SUBWOOFER_CODE_BASE))
            .filter(|count| (1..=4).contains(count)),
    })
}

/// Groups of the LFE distribution, in the order of the index. The indexes
/// 2 and 5 are not used.
const LFE_DISTRIBUTION_ORDER: [Option<SpeakerGroup>; 7] = [
    Some(SpeakerGroup::Front),
    Some(SpeakerGroup::Center),
    None,
    Some(SpeakerGroup::Surround),
    Some(SpeakerGroup::SurroundBack),
    None,
    Some(SpeakerGroup::FrontWide),
];

/// A value of the LFE distribution that means no LFE for the group.
pub const LFE_DISTRIBUTION_OFF: i32 = 99;

/// Which low frequencies go to the subwoofer outputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SubwooferOutputMode {
    /// Only the LFE channel.
    Lfe,
    /// The LFE channel and the low frequencies of the main channels.
    LfeAndMain,
}

impl SubwooferOutputMode {
    /// The mode for the number of the page: `1` or `2`.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Lfe),
            2 => Some(Self::LfeAndMain),
            _ => None,
        }
    }
}

/// The bass page of the web control: subwoofer output, the low pass filter of
/// the LFE channel and how much LFE the speakers get.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BassSettings {
    /// Which low frequencies go to the subwoofers.
    pub subwoofer_output: Option<SubwooferOutputMode>,
    /// The `display` attribute of the subwoofer output, see
    /// [`TableEntry::is_shown`].
    pub subwoofer_output_display: Option<u8>,
    /// Low pass filter of the LFE channel in Hz.
    pub lpf_for_lfe: Option<u16>,
    /// The `display` attribute of the low pass filter.
    pub lpf_for_lfe_display: Option<u8>,
    /// Frequencies that the receiver offers for the low pass filter.
    pub lpf_choices: Vec<u16>,
    /// LFE that each group of speakers gets in dB, from -20 to 0 in steps of
    /// 2, or [`LFE_DISTRIBUTION_OFF`]. The speakers need to be full range.
    pub lfe_distribution: Vec<TableEntry<SpeakerGroup>>,
    /// The `display` attribute of the whole distribution.
    pub lfe_distribution_display: Option<u8>,
}

impl BassSettings {
    /// Reads the bass page.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML or is an HTML page.
    pub fn parse(body: &[u8]) -> Result<Self, Error> {
        let node = root(body)?.node();
        let display = |child: &crate::xml::Child<'_>| {
            child
                .attribute("display")
                .and_then(|display| display.trim().parse().ok())
        };
        let output = node.child("SubwooferOutput");
        let lpf = node.child("LPFforLFE");
        let distribution = node.child("LFEDistribution");
        Ok(Self {
            subwoofer_output: number::<u8>(node, "SubwooferOutput")
                .and_then(SubwooferOutputMode::from_code),
            subwoofer_output_display: output.as_ref().and_then(display),
            lpf_for_lfe: number(node, "LPFforLFE"),
            lpf_for_lfe_display: lpf.as_ref().and_then(display),
            lpf_choices: node
                .child("SelectableValue")
                .and_then(|selectable| selectable.node().child("LPFforLFE"))
                .and_then(|lpf| lpf.node().child("List"))
                .map(|list| read_choices(list.node()))
                .unwrap_or_default()
                .into_iter()
                .filter_map(|hz| u16::try_from(hz).ok())
                .collect(),
            lfe_distribution: distribution
                .as_ref()
                .and_then(|d| d.node().child("List"))
                .map(|list| {
                    read_entries(list.node(), |index| {
                        LFE_DISTRIBUTION_ORDER.get(index).copied().flatten()
                    })
                })
                .unwrap_or_default(),
            lfe_distribution_display: distribution.as_ref().and_then(display),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DISTANCES: &[u8] =
        include_bytes!("../tests/data/xml/AVC-A10H-ajax-speakers-distances.xml");
    const CHANNEL_LEVELS: &[u8] =
        include_bytes!("../tests/data/xml/AVC-A10H-ajax-option-channel-levels.xml");
    const CROSSOVERS: &[u8] =
        include_bytes!("../tests/data/xml/AVC-A10H-ajax-speakers-crossovers.xml");
    const BASS: &[u8] = include_bytes!("../tests/data/xml/AVC-A10H-ajax-speakers-bass.xml");

    fn entry<K: Copy>(table: &SpeakerTable<K>, index: usize) -> (K, i32, Option<u8>) {
        let entry = table.entries[index];
        (entry.key, entry.value, entry.display)
    }

    #[test]
    fn distances_of_the_a10h() {
        let table = SpeakerTable::parse_distances(DISTANCES).unwrap();
        assert_eq!(table.unit, Some(3));
        // The test receiver has two subwoofers.
        assert_eq!(table.subwoofer_count, Some(2));
        assert_eq!(table.entries.len(), 13);
        // Unit 3 is milliseconds: the web page shows 2.5, 2.8, 2.7 and so on.
        let unit = table.distance_unit().unwrap();
        assert_eq!(unit, DistanceUnit::Milliseconds);
        assert!((unit.from_raw(58) - 5.8).abs() < 1e-6);
        assert_eq!(entry(&table, 0), (Channel::FrontLeft, 25, Some(3)));
        assert_eq!(entry(&table, 2), (Channel::Center, 27, Some(3)));
        assert_eq!(entry(&table, 6), (Channel::SurroundRight, 58, Some(3)));
        // The test receiver has front height and top middle speakers.
        assert_eq!(entry(&table, 9), (Channel::FrontHeightLeft, 6, Some(3)));
        assert_eq!(entry(&table, 11), (Channel::TopMiddleLeft, 54, Some(3)));
        assert_eq!(entry(&table, 12), (Channel::TopMiddleRight, 58, Some(3)));
        assert_eq!(table.choices.len(), 0);
    }

    #[test]
    fn channel_levels_of_the_a10h() {
        let table = SpeakerTable::parse_channel_levels(CHANNEL_LEVELS).unwrap();
        assert_eq!(table.entries.len(), 34);
        // The dial of the center was at -2 dB when the page was saved.
        assert_eq!(entry(&table, 0), (Channel::Center, -20, Some(3)));
        assert_eq!(entry(&table, 3), (Channel::FrontLeft, 0, Some(3)));
        assert_eq!(entry(&table, 9), (Channel::SurroundBack, 0, Some(1)));
        assert_eq!(entry(&table, 16), (Channel::TopMiddleLeft, 0, Some(3)));
        let shown: Vec<_> = table
            .entries
            .iter()
            .filter(|e| e.is_shown())
            .map(|e| e.key)
            .collect();
        // The setup of the test receiver: 13 channels like the web page.
        assert_eq!(shown.len(), 13);
        assert!(shown.contains(&Channel::FrontHeightRight));
        assert!(!shown.contains(&Channel::TopFrontLeft));
    }

    #[test]
    fn bass_page_of_the_a10h() {
        let bass = BassSettings::parse(BASS).unwrap();
        assert_eq!(bass.subwoofer_output, Some(SubwooferOutputMode::Lfe));
        assert_eq!(bass.subwoofer_output_display, Some(3));
        assert_eq!(bass.lpf_for_lfe, Some(120));
        assert_eq!(
            bass.lpf_choices,
            [70, 80, 90, 100, 110, 120, 150, 180, 200, 250]
        );
        assert_eq!(bass.lfe_distribution_display, Some(1));
        let groups: Vec<_> = bass.lfe_distribution.iter().map(|e| e.key).collect();
        assert_eq!(
            groups,
            [
                SpeakerGroup::Front,
                SpeakerGroup::Center,
                SpeakerGroup::Surround,
                SpeakerGroup::SurroundBack,
                SpeakerGroup::FrontWide,
            ]
        );
        assert!(
            bass.lfe_distribution
                .iter()
                .all(|e| e.value == LFE_DISTRIBUTION_OFF)
        );
        assert!(bass.lfe_distribution[0].is_editable());
        assert!(!bass.lfe_distribution[1].is_shown());
    }

    #[test]
    fn a_bass_page_with_missing_parts_is_empty() {
        let bass = BassSettings::parse(b"<Bass><LPFforLFE>x</LPFforLFE></Bass>").unwrap();
        assert_eq!(bass.lpf_for_lfe, None);
        assert_eq!(bass.subwoofer_output, None);
        assert_eq!(bass.lpf_choices.len(), 0);
        assert_eq!(bass.lfe_distribution.len(), 0);
        assert!(BassSettings::parse(b"<html/>").is_err());
        assert_eq!(
            SubwooferOutputMode::from_code(2),
            Some(SubwooferOutputMode::LfeAndMain)
        );
        assert_eq!(SubwooferOutputMode::from_code(0), None);
    }

    #[test]
    fn distance_units() {
        assert_eq!(DistanceUnit::from_code(1), Some(DistanceUnit::Meters));
        assert_eq!(DistanceUnit::from_code(2), Some(DistanceUnit::Feet));
        assert_eq!(DistanceUnit::from_code(0), None);
        assert!((DistanceUnit::Meters.from_raw(250) - 2.5).abs() < 1e-6);
        assert!((DistanceUnit::Feet.from_raw(305) - 10.0).abs() < 0.01);
        assert_eq!(DistanceUnit::Feet.symbol(), "ft");
        let entry = |display| TableEntry {
            key: (),
            value: 0,
            display,
        };
        assert!(entry(Some(3)).is_shown() && entry(Some(3)).is_editable());
        assert!(entry(Some(2)).is_shown() && !entry(Some(2)).is_editable());
        assert!(!entry(Some(1)).is_shown() && !entry(None).is_shown());
    }

    #[test]
    fn crossovers_of_the_a10h() {
        let table = SpeakerTable::parse_crossovers(CROSSOVERS).unwrap();
        assert_eq!(table.entries.len(), 6);
        assert_eq!(entry(&table, 0), (SpeakerGroup::Front, 0, Some(3)));
        assert_eq!(entry(&table, 3), (SpeakerGroup::SurroundBack, 80, Some(3)));
        assert_eq!(entry(&table, 4), (SpeakerGroup::FrontHeight, 80, Some(3)));
        assert_eq!(entry(&table, 5), (SpeakerGroup::TopMiddle, 80, Some(3)));
        assert_eq!(table.choices.len(), 13);
        assert_eq!(table.choices[..3], [0, 40, 60]);
    }

    #[test]
    fn unknown_indexes_and_bad_values_are_skipped() {
        let xml = br#"<Distances><List><Speaker index="0">5</Speaker>
            <Speaker index="99">1</Speaker><Speaker index="1">x</Speaker>
            <Speaker>3</Speaker></List></Distances>"#;
        let table = SpeakerTable::parse_distances(xml).unwrap();
        assert_eq!(table.entries.len(), 1);
        assert_eq!(table.unit, None);
        assert_eq!(table.subwoofer_count, None);
        for code in ["4", "10", "x"] {
            let xml = format!(
                "<D><SubwooferType><CurrentNumber>{code}</CurrentNumber></SubwooferType></D>"
            );
            let table = SpeakerTable::parse_distances(xml.as_bytes()).unwrap();
            assert_eq!(table.subwoofer_count, None, "{code}");
        }
    }

    #[test]
    fn bad_bodies_are_errors() {
        assert!(SpeakerTable::parse_distances(b"").is_err());
        assert!(SpeakerTable::parse_distances(b"<html></html>").is_err());
        assert!(SpeakerTable::parse_crossovers(b"<a><b></a>").is_err());
    }

    #[test]
    fn the_number_of_entries_is_limited() {
        let mut xml = String::from("<Crossovers><List>");
        for _ in 0..200 {
            xml.push_str(r#"<Speaker index="0">80</Speaker>"#);
        }
        xml.push_str("</List></Crossovers>");
        let table = SpeakerTable::parse_crossovers(xml.as_bytes()).unwrap();
        assert_eq!(table.entries.len(), MAX_ENTRIES);
    }
}
