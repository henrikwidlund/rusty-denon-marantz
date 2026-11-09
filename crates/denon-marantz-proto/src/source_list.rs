//! The input sources of a receiver: their names, if they are in use and the
//! physical inputs that they use. Telnet reports the names and if a source is
//! in use. The other values come from the web interface.

use crate::input_pages::{InputAssignment, SourceNameEntry, SourceNames};

/// One input source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceInfo {
    /// Telnet name, for example `SAT/CBL`.
    pub code: String,
    /// Name that is shown for the source, for example `Apple TV`. `None` if
    /// the receiver has not reported it, or reports an empty name.
    pub name: Option<String>,
    /// The source is in use. `false` if it is hidden. `None` if the receiver
    /// has not reported it.
    pub in_use: Option<bool>,
    /// The name of the source when nothing is changed, for example `Media
    /// Player`. `None` if the receiver has not reported it.
    pub default_name: Option<String>,
    /// The name that the device on the input reported itself, for example
    /// `UDP-203`. `None` if there is none or the receiver has not reported it.
    pub auto_name: Option<String>,
    /// The HDMI input of the source, for example `2`. `None` if it has none or
    /// the receiver has not reported it.
    pub hdmi_input: Option<String>,
    /// The digital input of the source, for example `COAX1` or `OPT2`.
    pub digital_input: Option<String>,
    /// The analog input of the source, for example `1`.
    pub analog_input: Option<String>,
    /// The video input of the source, for example `VIDEO1` or `COMP`.
    pub video_input: Option<String>,
}

impl SourceInfo {
    fn new(code: &str) -> Self {
        Self {
            code: code.to_owned(),
            name: None,
            in_use: None,
            default_name: None,
            auto_name: None,
            hdmi_input: None,
            digital_input: None,
            analog_input: None,
            video_input: None,
        }
    }
}

/// The sources of a receiver, in the order that the receiver reports them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceList {
    sources: Vec<SourceInfo>,
    max_name_length: Option<u8>,
}

impl SourceList {
    /// The sources, in the order that the receiver reports them.
    #[must_use]
    pub fn as_slice(&self) -> &[SourceInfo] {
        &self.sources
    }

    /// The source with this telnet name.
    #[must_use]
    pub fn get(&self, code: &str) -> Option<&SourceInfo> {
        self.sources.iter().find(|source| source.code == code)
    }

    /// The longest name that the receiver accepts for a source, in characters.
    /// `None` if the receiver has not reported it.
    #[must_use]
    pub const fn max_name_length(&self) -> Option<u8> {
        self.max_name_length
    }

    /// The name that is shown for a source, if the receiver reported one.
    #[must_use]
    pub fn name(&self, code: &str) -> Option<&str> {
        self.get(code)?.name.as_deref()
    }

    /// Applies the text of an `SS` event, for example `FUNMPLAY Apple TV` or
    /// `SODTV DEL`.
    ///
    /// Returns `None` if the event is not about sources. Otherwise returns
    /// true if the list changed.
    pub(crate) fn apply_ss(&mut self, parameter: &str) -> Option<bool> {
        if let Some(rest) = parameter.strip_prefix("FUN") {
            // `FUNGDN` and `FUNGAR` are lists of other names, not sources.
            if rest.starts_with("GDN") || rest.starts_with("GAR") {
                return None;
            }
            let (code, name) = split_code(rest)?;
            Some(self.set_name(code, Some(name)))
        } else if let Some(rest) = parameter.strip_prefix("SOD") {
            let (code, value) = split_code(rest)?;
            Some(match value.trim_end() {
                "USE" => self.set_in_use(code, true),
                "DEL" => self.set_in_use(code, false),
                _ => false,
            })
        } else {
            None
        }
    }

    /// Sets the name of a source. Spaces at the end and an empty name mean no
    /// name. Returns true if the list changed.
    pub(crate) fn set_name(&mut self, code: &str, name: Option<&str>) -> bool {
        let name = name.map(str::trim_end).filter(|name| !name.is_empty());
        self.source(code, |source| set_text(&mut source.name, name))
    }

    /// Sets if a source is in use. Returns true if the list changed.
    pub(crate) fn set_in_use(&mut self, code: &str, in_use: bool) -> bool {
        self.source(code, |source| {
            let changed = source.in_use != Some(in_use);
            source.in_use = Some(in_use);
            changed
        })
    }

    /// Stores the physical inputs of the sources of the input assign page.
    /// A source that the list does not know is added. An assignment without a
    /// telnet source name is ignored. Returns true if the list changed.
    pub(crate) fn set_assignments(&mut self, assignments: &[InputAssignment]) -> bool {
        let mut changed = false;
        for assignment in assignments {
            let Some(code) = assignment.source else {
                continue;
            };
            changed |= self.source(code, |source| {
                let mut changed = set_text(&mut source.hdmi_input, assignment.hdmi.as_deref());
                changed |= set_text(&mut source.digital_input, assignment.digital.as_deref());
                changed |= set_text(&mut source.analog_input, assignment.analog.as_deref());
                changed |= set_text(&mut source.video_input, assignment.video.as_deref());
                changed
            });
        }
        changed
    }

    /// Stores the default and automatic names of the source names page, and
    /// the longest name. The names that the user gave are not used: telnet
    /// reports them. Returns true if the list changed.
    pub(crate) fn set_name_details(&mut self, names: &SourceNames) -> bool {
        let mut changed = self.max_name_length != names.max_length;
        self.max_name_length = names.max_length;
        for SourceNameEntry {
            source,
            default_name,
            auto_name,
            ..
        } in &names.entries
        {
            let Some(code) = source else {
                continue;
            };
            changed |= self.source(code, |source| {
                let mut changed = set_text(&mut source.default_name, default_name.as_deref());
                changed |= set_text(&mut source.auto_name, auto_name.as_deref());
                changed
            });
        }
        changed
    }

    /// Updates a source, and adds it first if the list has none with this
    /// code. Returns true if the list changed.
    fn source(&mut self, code: &str, update: impl FnOnce(&mut SourceInfo) -> bool) -> bool {
        let found = self.sources.iter().position(|source| source.code == code);
        let is_new = found.is_none();
        let index = found.unwrap_or_else(|| {
            self.sources.push(SourceInfo::new(code));
            self.sources.len() - 1
        });
        update(&mut self.sources[index]) || is_new
    }
}

/// Splits `MPLAY Apple TV` into the source code and the rest. `None` for the
/// end of a list (`FUN END`) and for text that has no code.
fn split_code(text: &str) -> Option<(&str, &str)> {
    let (code, rest) = text.split_once(' ')?;
    (!code.is_empty()).then_some((code, rest))
}

/// Replaces the text. Returns true if it was different.
fn set_text(slot: &mut Option<String>, value: Option<&str>) -> bool {
    if slot.as_deref() == value {
        return false;
    }
    match (slot.as_mut(), value) {
        (Some(buffer), Some(value)) => {
            buffer.clear();
            buffer.push_str(value);
        }
        (_, value) => *slot = value.map(str::to_owned),
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_follow_events() {
        let mut list = SourceList::default();
        assert_eq!(list.apply_ss("FUNMPLAY Apple TV        "), Some(true));
        assert_eq!(list.apply_ss("FUNMPLAY Apple TV        "), Some(false));
        assert_eq!(list.apply_ss("FUNSAT/CBL TV"), Some(true));
        assert_eq!(list.name("MPLAY"), Some("Apple TV"));
        assert_eq!(list.name("SAT/CBL"), Some("TV"));
        assert_eq!(list.name("CD"), None);
        assert_eq!(list.apply_ss("FUNMPLAY Media"), Some(true));
        assert_eq!(list.name("MPLAY"), Some("Media"));
    }

    #[test]
    fn order_is_the_order_of_the_receiver() {
        let mut list = SourceList::default();
        for line in ["FUNBD Player", "SODTV DEL", "FUNTV TV Audio", "SODBD USE"] {
            list.apply_ss(line);
        }
        let codes: Vec<_> = list.as_slice().iter().map(|s| s.code.as_str()).collect();
        assert_eq!(codes, ["BD", "TV"]);
    }

    #[test]
    fn use_flag_follows_events() {
        let mut list = SourceList::default();
        assert_eq!(list.apply_ss("SODTV DEL"), Some(true));
        assert_eq!(list.apply_ss("SODTV DEL"), Some(false));
        assert_eq!(list.get("TV").unwrap().in_use, Some(false));
        assert_eq!(list.apply_ss("SODTV USE"), Some(true));
        assert_eq!(list.get("TV").unwrap().in_use, Some(true));
        // A flag that is not known changes nothing.
        assert_eq!(list.apply_ss("SODTV ???"), Some(false));
        assert_eq!(list.get("TV").unwrap().in_use, Some(true));
    }

    #[test]
    fn details_of_the_web_pages_join_the_sources() {
        let assignments = InputAssignment::parse(
            br#"<InputAssign><Source index="5"><Name>Apple TV</Name><HDMI>4</HDMI></Source>
            <Source index="2"><Name>Unknown</Name><HDMI>4</HDMI></Source>
            <SelectionList><HDMI><List><Item index="4">2</Item></List></HDMI></SelectionList>
            </InputAssign>"#,
        )
        .unwrap();
        let names = SourceNames::parse(
            br#"<SourceRename><CharacterMax>16</CharacterMax><Source index="5">
            <Default>Media Player</Default><Rename>Apple TV</Rename><Auto>Apple TV 4K</Auto>
            </Source></SourceRename>"#,
        )
        .unwrap();
        let mut list = SourceList::default();
        list.apply_ss("FUNMPLAY Apple TV");
        assert!(list.set_assignments(&assignments));
        assert!(!list.set_assignments(&assignments));
        assert!(list.set_name_details(&names));
        assert!(!list.set_name_details(&names));
        let source = list.get("MPLAY").unwrap();
        assert_eq!(source.name.as_deref(), Some("Apple TV"));
        assert_eq!(source.hdmi_input.as_deref(), Some("2"));
        assert_eq!(source.digital_input, None);
        assert_eq!(source.default_name.as_deref(), Some("Media Player"));
        assert_eq!(source.auto_name.as_deref(), Some("Apple TV 4K"));
        assert_eq!(list.max_name_length(), Some(16));
        // The source with an unknown index is left out.
        assert_eq!(list.as_slice().len(), 1);
        // A change on the page is a change of the list.
        let moved = InputAssignment::parse(
            br#"<InputAssign><Source index="5"><HDMI>5</HDMI></Source>
            <SelectionList><HDMI><List><Item index="5">3</Item></List></HDMI></SelectionList>
            </InputAssign>"#,
        )
        .unwrap();
        assert!(list.set_assignments(&moved));
        assert_eq!(list.get("MPLAY").unwrap().hdmi_input.as_deref(), Some("3"));
    }

    #[test]
    fn empty_name_is_no_name() {
        let mut list = SourceList::default();
        list.apply_ss("FUNCD CD");
        assert_eq!(list.apply_ss("FUNCD      "), Some(true));
        assert_eq!(list.name("CD"), None);
        assert!(list.get("CD").is_some());
    }

    #[test]
    fn other_lines_are_not_sources() {
        let mut list = SourceList::default();
        assert_eq!(list.apply_ss("FUN END"), None);
        assert_eq!(list.apply_ss("SOD END"), None);
        assert_eq!(list.apply_ss("FUNGDNBD Blu-ray"), None);
        assert_eq!(list.apply_ss("FUNGARBD UDP-203"), None);
        assert_eq!(list.apply_ss("INFSIGRES I4K50"), None);
        assert_eq!(list.apply_ss("FUNBD"), None);
        assert_eq!(list.as_slice(), []);
    }
}
