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
    /// The `display` attribute of the page. `3` means the value can be
    /// changed. The meaning of the other numbers is not documented.
    pub display: Option<u8>,
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
    /// The `Unit` of the page. Only the distances page has it.
    pub unit: Option<u8>,
    /// The `MsecMax` of the page. Only the distances page has it.
    pub max_msec: Option<i32>,
}

impl SpeakerTable<Channel> {
    /// Reads the distances page.
    ///
    /// The unit of the values is not documented: see [`Self::unit`].
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML or is an HTML page.
    pub fn parse_distances(body: &[u8]) -> Result<Self, Error> {
        parse(body, |index| DISTANCE_ORDER.get(index).copied())
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
        assert_eq!(entry(&table, 0), (Channel::FrontLeft, 25, Some(3)));
        assert_eq!(entry(&table, 2), (Channel::Center, 27, Some(3)));
        // The test receiver has front height and top middle speakers.
        assert_eq!(entry(&table, 9), (Channel::FrontHeightLeft, 6, Some(3)));
        assert_eq!(entry(&table, 11), (Channel::TopMiddleLeft, 54, Some(3)));
        assert_eq!(entry(&table, 12), (Channel::TopMiddleRight, 58, Some(3)));
        assert_eq!(table.choices.len(), 0);
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
