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

/// Path of the channel level page, the dials of the web control. The levels
/// are a temporary change on top of the speaker levels.
pub const CHANNEL_LEVELS_PATH: &str = "/ajax/option/get_config?type=3";

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
    /// The `mode` attribute of the page, for example `diracmenulock`. The
    /// receiver locks the values in this mode.
    pub mode: Option<String>,
    /// Entries in the order of the page.
    pub entries: Vec<TableEntry<K>>,
    /// Values that the page offers for the setting, for example crossover
    /// frequencies. Empty if the page has no list.
    pub choices: Vec<i32>,
    /// The `Unit` of the page. Only the distances page has it. See
    /// [`SpeakerTable::distance_unit`].
    pub unit: Option<u8>,
    /// The `MsecMax` of the page. Only the distances page has it.
    pub max_msec: Option<i32>,
}

/// The speaker tables of the web control pages that a receiver gave.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpeakerTables {
    /// Distances, if the receiver has the page.
    pub distances: Option<SpeakerTable<Channel>>,
    /// Crossovers, if the receiver has the page.
    pub crossovers: Option<SpeakerTable<SpeakerGroup>>,
    /// Channel levels, if the receiver has the page.
    pub channel_levels: Option<SpeakerTable<Channel>>,
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
    /// cannot be used, for example while a Dirac filter is on.
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

fn parse<K>(body: &[u8], key: impl Fn(usize) -> Option<K>) -> Result<SpeakerTable<K>, Error> {
    check_well_formed(body)?;
    let root = Node::new(body)
        .children()
        .next()
        .ok_or(Error::HtmlResponse)?;
    if root.is("html") {
        return Err(Error::HtmlResponse);
    }
    let node = root.node();
    let number = |name: &str| {
        node.child(name)
            .and_then(|child| child.node().text())
            .and_then(|text| text.trim().parse().ok())
    };
    let mut table = SpeakerTable {
        mode: root.attribute("mode").map(std::borrow::Cow::into_owned),
        entries: Vec::new(),
        choices: Vec::new(),
        unit: number("Unit").and_then(|unit| u8::try_from(unit).ok()),
        max_msec: number("MsecMax"),
    };
    if let Some(list) = node.child("List") {
        for speaker in list.node().children().filter(|c| c.is("Speaker")) {
            let index = speaker
                .attribute("index")
                .and_then(|index| index.trim().parse::<usize>().ok());
            let value = speaker
                .node()
                .text()
                .and_then(|text| text.trim().parse().ok());
            if let (Some(key), Some(value)) = (index.and_then(&key), value) {
                if table.entries.len() == MAX_ENTRIES {
                    break;
                }
                table.entries.push(TableEntry {
                    key,
                    value,
                    display: speaker
                        .attribute("display")
                        .and_then(|display| display.trim().parse().ok()),
                });
            }
        }
    }
    if let Some(items) = node
        .child("SelectableValue")
        .and_then(|selectable| selectable.node().child("List"))
    {
        table.choices.extend(
            items
                .node()
                .children()
                .filter(|c| c.is("Item"))
                .filter_map(|item| item.node().text()?.trim().parse::<i32>().ok())
                .take(MAX_ENTRIES),
        );
    }
    Ok(table)
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

    fn entry<K: Copy>(table: &SpeakerTable<K>, index: usize) -> (K, i32, Option<u8>) {
        let entry = table.entries[index];
        (entry.key, entry.value, entry.display)
    }

    #[test]
    fn distances_of_the_a10h() {
        let table = SpeakerTable::parse_distances(DISTANCES).unwrap();
        assert_eq!(table.mode.as_deref(), Some("diracmenulock"));
        assert_eq!(table.unit, Some(3));
        assert_eq!(table.max_msec, Some(50));
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
        assert_eq!(table.mode.as_deref(), Some("diracmenulock"));
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
        assert_eq!(table.mode, None);
        assert_eq!(table.unit, None);
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
