//! Two speaker pages of the web interface: the playback of two-channel
//! sources and the assignment of the subwoofer XLR outputs.
//!
//! The pages are `/ajax/speakers/get_config?type=N` on port 11080 and do not
//! send changes. The meaning of the numbers is the one that the web interface
//! shows.

use std::collections::HashSet;

use crate::channel::Channel;
use crate::error::Error;
use crate::speaker_tables::{DistanceUnit, TableEntry, number, read_entries, root};
use crate::xml::Node;

/// Path of the playback page of two-channel sources.
pub const PLAYBACK_2CH_PATH: &str = "/ajax/speakers/get_config?type=9";

/// Path of the XLR output assignment page.
pub const XLR_ASSIGN_PATH: &str = "/ajax/speakers/get_config?type=12";

/// Most outputs of the XLR page that are kept.
const MAX_OUTPUTS: usize = 8;

/// Most choices of an XLR output that are kept.
const MAX_CHOICES: usize = 8;

code_enum! {
    /// Where the bass of a two-channel source goes.
    pub enum SubwooferOutput2Ch {
        Lfe = "1", "LFE";
        LfeAndMain = "2", "LFE + Main";
    }
}

code_enum! {
    /// The target of an XLR output.
    pub enum XlrTarget {
        Subwoofer = "1", "Subwoofer";
        FrontLeft = "2", "Front left";
        FrontRight = "3", "Front right";
        Center = "4", "Center";
        None = "5", "None";
    }
}

/// How much of the LFE channel goes to the main speakers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfeDistribution {
    /// None of it.
    Off,
    /// The level in whole dB, from -20 to 0.
    Db(i8),
}

/// How the receiver plays two-channel sources.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Playback2Ch {
    /// The receiver chooses the setup (`true`) or it is set by hand.
    pub automatic: Option<bool>,
    /// A subwoofer is used.
    pub subwoofer: Option<bool>,
    /// Where the bass goes.
    pub subwoofer_output: Option<SubwooferOutput2Ch>,
    /// The crossover in Hz. `0` is full band.
    pub crossover_hz: Option<u16>,
    /// How much of the LFE channel goes to the main speakers.
    pub lfe_distribution: Option<LfeDistribution>,
    /// The unit of the distances.
    pub distance_unit: Option<DistanceUnit>,
    /// The distances of the front left and the front right speaker, in the
    /// unit of `distance_unit` as in [`SpeakerTable`](crate::SpeakerTable).
    pub distances: Vec<TableEntry<Channel>>,
    /// The levels of the front speakers, in tenths of a dB.
    pub levels: Vec<TableEntry<Channel>>,
}

impl Playback2Ch {
    /// Reads the playback page.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML, is an HTML page, is not
    /// the playback page, or has no value that is known. A page without
    /// content must not replace what the state has.
    pub fn parse(body: &[u8]) -> Result<Self, Error> {
        let root = root(body)?;
        if !root.is("Playback2Ch") {
            return Err(Error::UnexpectedDocument {
                expected: "Playback2Ch",
            });
        }
        let node = root.node();
        let front = |index: usize| match index {
            0 => Some(Channel::FrontLeft),
            1 => Some(Channel::FrontRight),
            _ => None,
        };
        let entries = |name: &str| {
            let mut entries = node
                .child(name)
                .map_or_default(|list| read_entries(list.node(), front));
            // A speaker is listed once, and the first entry counts.
            let mut seen = HashSet::new();
            entries.retain(|entry| seen.insert(entry.key));
            entries
        };
        let page = Self {
            automatic: match number::<u8>(node, "Setting") {
                Some(1) => Some(true),
                Some(2) => Some(false),
                _ => None,
            },
            subwoofer: match number::<u8>(node, "Subwoofer") {
                Some(4) => Some(true),
                Some(5) => Some(false),
                _ => None,
            },
            subwoofer_output: child_with(node, "SubwooferOutput", SubwooferOutput2Ch::from_code),
            crossover_hz: number(node, "Crossover"),
            lfe_distribution: match number::<i32>(node, "LFEDistribution") {
                Some(99) => Some(LfeDistribution::Off),
                Some(db @ -20..=0) => i8::try_from(db).ok().map(LfeDistribution::Db),
                _ => None,
            },
            distance_unit: number::<u8>(node, "Unit").and_then(DistanceUnit::from_code),
            distances: entries("DistanceList"),
            levels: entries("LevelList"),
        };
        if page == Self::default() {
            return Err(Error::UnexpectedDocument {
                expected: "a page with a known value",
            });
        }
        Ok(page)
    }
}

/// One XLR output of the subwoofers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XlrOutput {
    /// The number of the output, from 1. The page names them after the
    /// subwoofers.
    pub output: u8,
    /// What the output carries. `None` if the page has no known value.
    pub target: Option<XlrTarget>,
    /// What the receiver lets the user choose.
    pub choices: Vec<XlrTarget>,
}

/// What the XLR outputs carry. Only models that have XLR subwoofer outputs use
/// them. Other models still answer the page, with the value `0` for each
/// output, so [`XlrOutput::target`] is `None` there.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct XlrAssign {
    /// The outputs, in the order of the page.
    pub outputs: Vec<XlrOutput>,
    /// A tactile transducer (bass shaker) is present.
    pub bass_shaker: Option<bool>,
}

impl XlrAssign {
    /// Reads the XLR output assignment page.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML, is an HTML page, is not
    /// the XLR page, or lists no output. A page without content must not
    /// replace what the state has.
    pub fn parse(body: &[u8]) -> Result<Self, Error> {
        let root = root(body)?;
        if !root.is("SwXLROutputAssign") {
            return Err(Error::UnexpectedDocument {
                expected: "SwXLROutputAssign",
            });
        }
        let node = root.node();
        let mut outputs = Vec::new();
        let mut seen = HashSet::new();
        for speaker in node.children().filter(|c| c.is("Speaker")) {
            // The page counts from 0. An index that cannot be shifted is not
            // a real output.
            let Some(output) = speaker
                .attribute("index")
                .and_then(|index| index.trim().parse::<u8>().ok())
                .and_then(|index| index.checked_add(1))
            else {
                continue;
            };
            if !seen.insert(output) {
                continue;
            }
            if outputs.len() == MAX_OUTPUTS {
                break;
            }
            let content = speaker.node();
            outputs.push(XlrOutput {
                output,
                target: child_with(content, "Value", XlrTarget::from_code),
                choices: content
                    .children()
                    .filter(|c| c.is("List"))
                    .filter_map(|c| XlrTarget::from_code(c.node().text()?.trim()))
                    .take(MAX_CHOICES)
                    .collect(),
            });
        }
        if outputs.is_empty() {
            return Err(Error::UnexpectedDocument {
                expected: "a list of XLR outputs",
            });
        }
        Ok(Self {
            outputs,
            bass_shaker: match number::<u8>(node, "BassShaker") {
                Some(1) => Some(true),
                Some(0) => Some(false),
                _ => None,
            },
        })
    }
}

/// Reads the text of the child `name` with `read`, without a copy.
fn child_with<T>(node: Node<'_>, name: &str, read: impl FnOnce(&str) -> Option<T>) -> Option<T> {
    let text = node.child(name)?.node().text()?;
    read(text.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playback_page_of_the_a10h() {
        let page = Playback2Ch::parse(include_bytes!(
            "../tests/data/xml/AVC-A10H-web-speakers-playback-2ch.xml"
        ))
        .unwrap();
        assert_eq!(page.automatic, Some(true));
        assert_eq!(page.subwoofer, Some(true));
        assert_eq!(page.subwoofer_output, Some(SubwooferOutput2Ch::Lfe));
        assert_eq!(page.crossover_hz, Some(0));
        assert_eq!(page.lfe_distribution, Some(LfeDistribution::Off));
        assert_eq!(page.distance_unit, Some(DistanceUnit::Milliseconds));
        assert_eq!(page.distances.len(), 2);
        assert_eq!(page.distances[0].key, Channel::FrontLeft);
        assert_eq!(page.distances[0].value, 25);
        assert_eq!(page.levels[1].key, Channel::FrontRight);
        assert_eq!(page.levels[1].value, -5);
        // Dirac locks these, so they are shown but not editable.
        assert!(!page.levels[0].is_editable());
    }

    #[test]
    fn xlr_page_of_the_a10h() {
        let page = XlrAssign::parse(include_bytes!(
            "../tests/data/xml/AVC-A10H-web-speakers-xlr-assign.xml"
        ))
        .unwrap();
        assert_eq!(page.outputs.len(), 4);
        assert_eq!(page.outputs[0].output, 1);
        // The value 0 is not one of the targets.
        assert_eq!(page.outputs[0].target, None);
        assert_eq!(page.outputs[0].choices.len(), 5);
        assert_eq!(page.outputs[0].choices[0], XlrTarget::Subwoofer);
        assert_eq!(page.outputs[2].choices.len(), 4);
        assert_eq!(page.bass_shaker, Some(false));
    }

    #[test]
    fn a_page_without_content_is_an_error() {
        // The right root, but nothing usable: it must not clear what is known.
        assert!(Playback2Ch::parse(b"<Playback2Ch/>").is_err());
        assert!(Playback2Ch::parse(b"<Playback2Ch><Setting>9</Setting></Playback2Ch>").is_err());
        assert!(XlrAssign::parse(b"<SwXLROutputAssign/>").is_err());
    }

    #[test]
    fn outputs_and_speakers_are_listed_once() {
        let page = XlrAssign::parse(
            b"<SwXLROutputAssign>\
              <Speaker index=\"0\"><Value>1</Value></Speaker>\
              <Speaker index=\"0\"><Value>2</Value></Speaker>\
              <Speaker index=\"255\"><Value>3</Value></Speaker>\
              <Speaker index=\"254\"><Value>4</Value></Speaker>\
              </SwXLROutputAssign>",
        )
        .unwrap();
        // The second entry of index 0 is dropped, and 255 cannot be shifted.
        let outputs: Vec<_> = page.outputs.iter().map(|o| (o.output, o.target)).collect();
        assert_eq!(
            outputs,
            [
                (1, Some(XlrTarget::Subwoofer)),
                (255, Some(XlrTarget::Center))
            ]
        );
        let playback = Playback2Ch::parse(
            b"<Playback2Ch><Setting>1</Setting><DistanceList>\
              <Speaker index=\"0\">25</Speaker><Speaker index=\"0\">30</Speaker>\
              </DistanceList></Playback2Ch>",
        )
        .unwrap();
        assert_eq!(playback.distances.len(), 1);
        assert_eq!(playback.distances[0].value, 25);
    }

    #[test]
    fn another_page_and_html_are_errors() {
        assert!(Playback2Ch::parse(b"<SwXLROutputAssign/>").is_err());
        assert!(XlrAssign::parse(b"<html><body>no</body></html>").is_err());
        // One value that is known is enough, and an unknown number is `None`.
        let odd = Playback2Ch::parse(
            b"<Playback2Ch><LFEDistribution>7</LFEDistribution><Setting>1</Setting></Playback2Ch>",
        )
        .unwrap();
        assert_eq!(odd.lfe_distribution, None);
        assert_eq!(odd.automatic, Some(true));
    }
}
