//! Inputs from the web interface: which physical input each source uses,
//! and the names of the sources. The values go into the
//! [`SourceList`](crate::SourceList) of the state.
//!
//! The pages are `/ajax/inputs/get_config?type=N` on port 11080. They list
//! sources by an index. The index is converted to the telnet source name for
//! the sources that are known. Other sources keep only the index.

use crate::error::Error;
use crate::reading::Reading;
use crate::speaker_tables::{number, root};
use crate::xml::Node;

/// Path of the input assign page.
pub const INPUT_ASSIGN_PATH: &str = "/ajax/inputs/get_config?type=2";
/// Path of the source names page.
pub const SOURCE_NAMES_PATH: &str = "/ajax/inputs/get_config?type=3";
/// Path of the source levels page.
pub const SOURCE_LEVELS_PATH: &str = "/ajax/inputs/get_config?type=5";

/// The level that the receiver adds to the inputs of one kind, in whole dB
/// from -12 to 12.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SourceLevels {
    /// The level of the analog inputs.
    pub analog_db: Option<i8>,
    /// The level of the digital inputs.
    pub digital_db: Option<i8>,
}

impl SourceLevels {
    /// Reads the source levels page.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML, is an HTML page, is not
    /// the source levels page, or has no level in range.
    pub fn parse(body: &[u8]) -> Result<Self, Error> {
        let root = root(body)?;
        if !root.is("SourceLevels") {
            return Err(Error::UnexpectedDocument {
                expected: "SourceLevels",
            });
        }
        let node = root.node();
        let level = |name: &str| number::<i8>(node, name).filter(|db| (-12..=12).contains(db));
        let levels = Self {
            analog_db: level("AnalogInputs"),
            digital_db: level("DigitalInputs"),
        };
        if levels == Self::default() {
            // A page without a level must not replace what the state has.
            return Err(Error::UnexpectedDocument {
                expected: "a page with a level",
            });
        }
        Ok(levels)
    }
}

/// Most sources of a page that are kept.
const MAX_SOURCES: usize = 32;

/// Telnet source name of an index of the web interface.
#[must_use]
pub const fn source_for_index(index: u8) -> Option<&'static str> {
    Some(match index {
        1 => "SAT/CBL",
        3 => "BD",
        4 => "GAME1",
        5 => "MPLAY",
        6 => "TV",
        7 => "AUX1",
        8 => "AUX2",
        9 => "CD",
        10 => "PHONO",
        11 => "TUNER",
        13 => "NET",
        14 => "AUX3",
        15 => "AUX4",
        16 => "AUX5",
        17 => "AUX6",
        18 => "AUX7",
        21 => "GAME2",
        _ => return None,
    })
}

/// The physical inputs that a source uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputAssignment {
    /// Index of the source on the web interface.
    pub index: u8,
    /// Telnet name of the source, if the index is known.
    pub source: Option<&'static str>,
    /// Name of the source.
    pub name: String,
    /// HDMI input, for example `2`. `None` if no HDMI input is assigned.
    pub hdmi: Option<String>,
    /// Digital input, for example `COAX1` or `OPT2`.
    pub digital: Option<String>,
    /// Analog input, for example `1`.
    pub analog: Option<String>,
    /// Video input, for example `VIDEO1` or `COMP`.
    pub video: Option<String>,
}

impl InputAssignment {
    /// Reads the input assign page.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML, is an HTML page, is not
    /// the input assign page, or has no source. A page without content must
    /// not replace what the state has.
    pub fn parse(body: &[u8]) -> Result<Vec<Self>, Error> {
        let root = root(body)?;
        if !root.is("InputAssign") {
            return Err(Error::UnexpectedDocument {
                expected: "InputAssign",
            });
        }
        let node = root.node();
        let selection = node.child("SelectionList").map(|list| list.node());
        // The page gives the number of a list item, the text of the item is
        // the name of the input.
        let label = |port: &str, element: &crate::xml::Child<'_>| -> Option<String> {
            let wanted: u32 = element.node().text()?.trim().parse().ok()?;
            let list = selection?.child(port)?.node().child("List")?;
            let item = list.node().children().find(|item| {
                item.attribute("index")
                    .and_then(|index| index.trim().parse::<u32>().ok())
                    == Some(wanted)
            })?;
            let text = item.node().text()?;
            let text = text.trim();
            (!text.is_empty() && text != "-").then(|| text.to_owned())
        };
        let mut assignments: Vec<Self> = Vec::new();
        for source in node.children().filter(|c| c.is("Source")) {
            let Some(index) = source
                .attribute("index")
                .and_then(|index| index.trim().parse::<u8>().ok())
            else {
                continue;
            };
            if assignments.iter().any(|known| known.index == index) {
                continue;
            }
            if assignments.len() == MAX_SOURCES {
                break;
            }
            let content = source.node();
            let port = |name: &str| content.child(name).and_then(|e| label(name, &e));
            assignments.push(Self {
                index,
                source: source_for_index(index),
                name: text_of(content, "Name").unwrap_or_default(),
                hdmi: port("HDMI"),
                digital: port("Digital"),
                analog: port("Analog"),
                video: port("Video"),
            });
        }
        if assignments.is_empty() {
            return Err(Error::UnexpectedDocument {
                expected: "InputAssign",
            });
        }
        Ok(assignments)
    }
}

/// The names of one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceNameEntry {
    /// Index of the source on the web interface.
    pub index: u8,
    /// Telnet name of the source, if the index is known.
    pub source: Option<&'static str>,
    /// The name of the source when nothing is changed, for example
    /// `Media Player`.
    pub default_name: Reading<String>,
    /// The name that the user gave, for example `Apple TV`.
    pub rename: Reading<String>,
    /// The name that the device on the input reported itself, for example
    /// `UDP-203`. It can be used as a name by the receiver.
    pub auto_name: Reading<String>,
}

/// The source names page.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceNames {
    /// Longest name that the receiver accepts, in characters.
    pub max_length: Option<u8>,
    /// The sources that have names on the page.
    pub entries: Vec<SourceNameEntry>,
}

impl SourceNames {
    /// Reads the source names page.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML, is an HTML page, is not
    /// the source name page, or has no source. A page without content must
    /// not replace what the state has.
    pub fn parse(body: &[u8]) -> Result<Self, Error> {
        let root = root(body)?;
        if !root.is("SourceRename") {
            return Err(Error::UnexpectedDocument {
                expected: "SourceRename",
            });
        }
        let node = root.node();
        let mut names = Self {
            max_length: number(node, "CharacterMax"),
            entries: Vec::new(),
        };
        for source in node.children().filter(|c| c.is("Source")) {
            let Some(index) = source
                .attribute("index")
                .and_then(|index| index.trim().parse::<u8>().ok())
            else {
                continue;
            };
            if names.entries.iter().any(|known| known.index == index) {
                continue;
            }
            if names.entries.len() == MAX_SOURCES {
                break;
            }
            let content = source.node();
            names.entries.push(SourceNameEntry {
                index,
                source: source_for_index(index),
                default_name: reading_of(content, "Default"),
                rename: reading_of(content, "Rename"),
                auto_name: reading_of(content, "Auto"),
            });
        }
        if names.entries.is_empty() {
            return Err(Error::UnexpectedDocument {
                expected: "SourceRename",
            });
        }
        Ok(names)
    }
}

/// The trimmed text of the child `name`. `None` if it is missing or empty.
fn text_of(node: Node<'_>, name: &str) -> Option<String> {
    let text = node.child(name)?.node().text()?;
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

/// Like [`text_of`], and [`Reading::Absent`] if the child is missing.
fn reading_of(node: Node<'_>, name: &str) -> Reading<String> {
    if node.child(name).is_none() {
        return Reading::Absent;
    }
    Reading::from_option(text_of(node, name))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASSIGN: &[u8] = include_bytes!("../tests/data/xml/AVC-A10H-web-inputs-assign.xml");
    const NAMES: &[u8] = include_bytes!("../tests/data/xml/AVC-A10H-web-inputs-names.xml");

    #[test]
    fn input_assign_of_the_a10h() {
        let assignments = InputAssignment::parse(ASSIGN).unwrap();
        assert_eq!(assignments.len(), 5);
        let apple = &assignments[0];
        assert_eq!(apple.index, 5);
        assert_eq!(apple.source, Some("MPLAY"));
        assert_eq!(apple.name, "Apple TV");
        // The page gives list item 4, which is HDMI input 2.
        assert_eq!(apple.hdmi.as_deref(), Some("2"));
        assert_eq!(apple.digital, None);
        assert_eq!(apple.analog, None);
        assert_eq!(apple.video, None);
        let hdmi: Vec<_> = assignments.iter().map(|a| a.hdmi.as_deref()).collect();
        assert_eq!(
            hdmi,
            [Some("2"), Some("3"), Some("4"), Some("5"), Some("6")]
        );
        assert_eq!(assignments[4].source, Some("AUX2"));
    }

    #[test]
    fn source_names_of_the_a10h() {
        let names = SourceNames::parse(NAMES).unwrap();
        assert_eq!(names.max_length, Some(16));
        assert_eq!(names.entries.len(), 5);
        let bluray = &names.entries[1];
        assert_eq!(bluray.source, Some("BD"));
        assert_eq!(
            bluray.default_name.as_value().map(String::as_str),
            Some("Blu-ray")
        );
        assert_eq!(
            bluray.rename.as_value().map(String::as_str),
            Some("Blu-ray")
        );
        assert_eq!(
            bluray.auto_name.as_value().map(String::as_str),
            Some("UDP-203")
        );
    }

    #[test]
    fn digital_analog_and_video_inputs_have_names() {
        let xml = br#"<InputAssign><Source index="9"><Name>CD</Name>
            <HDMI>1</HDMI><Digital>5</Digital><Analog>3</Analog><Video>8</Video></Source>
            <Source index="99"><Name>X</Name><HDMI>7</HDMI></Source>
            <Source><Name>No index</Name></Source>
            <SelectionList><HDMI><List><Item index="1">-</Item></List></HDMI>
            <Digital><List><Item index="5">COAX1</Item></List></Digital>
            <Analog><List><Item index="3">1</Item></List></Analog>
            <Video><List><Item index="8">COMP</Item></List></Video></SelectionList>
            </InputAssign>"#;
        let assignments = InputAssignment::parse(xml).unwrap();
        assert_eq!(assignments.len(), 2);
        let cd = &assignments[0];
        assert_eq!(cd.source, Some("CD"));
        assert_eq!(cd.hdmi, None);
        assert_eq!(cd.digital.as_deref(), Some("COAX1"));
        assert_eq!(cd.analog.as_deref(), Some("1"));
        assert_eq!(cd.video.as_deref(), Some("COMP"));
        // An unknown source keeps its index, and an item that the list does
        // not have gives no input.
        assert_eq!(assignments[1].source, None);
        assert_eq!(assignments[1].hdmi, None);
    }

    #[test]
    fn other_or_empty_pages_are_errors_and_repeated_sources_count_once() {
        assert!(InputAssignment::parse(b"<Other/>").is_err());
        assert!(InputAssignment::parse(b"<InputAssign/>").is_err());
        assert!(SourceNames::parse(b"<Other/>").is_err());
        assert!(
            SourceNames::parse(b"<SourceRename><CharacterMax>16</CharacterMax></SourceRename>")
                .is_err()
        );
        let xml = br#"<SourceRename><Source index="3"><Rename>First</Rename></Source>
            <Source index="3"><Rename>Second</Rename></Source></SourceRename>"#;
        let names = SourceNames::parse(xml).unwrap();
        assert_eq!(names.entries.len(), 1);
        assert_eq!(
            names.entries[0].rename.as_value().map(String::as_str),
            Some("First")
        );
        let xml = br#"<InputAssign><Source index="5"><Name>A</Name></Source>
            <Source index="5"><Name>B</Name></Source></InputAssign>"#;
        let list = InputAssignment::parse(xml).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "A");
    }

    #[test]
    fn indexes_and_empty_names() {
        assert_eq!(source_for_index(1), Some("SAT/CBL"));
        assert_eq!(source_for_index(21), Some("GAME2"));
        assert_eq!(source_for_index(2), None);
        let xml = b"<SourceRename><Source index=\"3\"><Default/><Rename> </Rename></Source></SourceRename>";
        let names = SourceNames::parse(xml).unwrap();
        assert_eq!(names.max_length, None);
        // An element without a text is empty, and a missing element is absent.
        assert_eq!(names.entries[0].default_name, Reading::Empty);
        assert_eq!(names.entries[0].rename, Reading::Empty);
        assert_eq!(names.entries[0].auto_name, Reading::Absent);
        assert!(SourceNames::parse(b"<html/>").is_err());
        assert!(InputAssignment::parse(b"").is_err());
    }

    #[test]
    fn source_levels_page() {
        let levels = SourceLevels::parse(include_bytes!(
            "../tests/data/xml/AVC-A10H-web-inputs-source-levels.xml"
        ))
        .unwrap();
        assert_eq!((levels.analog_db, levels.digital_db), (Some(0), Some(0)));
        let odd = SourceLevels::parse(
            b"<SourceLevels><AnalogInputs>-3</AnalogInputs><DigitalInputs>40</DigitalInputs></SourceLevels>",
        )
        .unwrap();
        assert_eq!((odd.analog_db, odd.digital_db), (Some(-3), None));
        assert!(SourceLevels::parse(b"<HideSources/>").is_err());
        // The right root without a level in range must not clear the state.
        assert!(SourceLevels::parse(b"<SourceLevels/>").is_err());
        assert!(
            SourceLevels::parse(b"<SourceLevels><AnalogInputs>99</AnalogInputs></SourceLevels>")
                .is_err()
        );
    }
}
