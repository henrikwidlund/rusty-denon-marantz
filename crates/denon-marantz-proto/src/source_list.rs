//! The input sources of a receiver: their names and if they are in use.
//! Telnet reports them.

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
}

/// The sources of a receiver, in the order that the receiver reports them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceList {
    sources: Vec<SourceInfo>,
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
            let name = Some(name.trim_end()).filter(|name| !name.is_empty());
            Some(self.source(code, |source| set_text(&mut source.name, name)))
        } else if let Some(rest) = parameter.strip_prefix("SOD") {
            let (code, value) = split_code(rest)?;
            let in_use = match value.trim_end() {
                "USE" => true,
                "DEL" => false,
                _ => return Some(false),
            };
            Some(self.source(code, |source| {
                let changed = source.in_use != Some(in_use);
                source.in_use = Some(in_use);
                changed
            }))
        } else {
            None
        }
    }

    /// Updates a source, and adds it first if the list has none with this
    /// code. Returns true if the list changed.
    fn source(&mut self, code: &str, update: impl FnOnce(&mut SourceInfo) -> bool) -> bool {
        let found = self.sources.iter().position(|source| source.code == code);
        let is_new = found.is_none();
        let index = found.unwrap_or_else(|| {
            self.sources.push(SourceInfo {
                code: code.to_owned(),
                name: None,
                in_use: None,
            });
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
        for line in ["FUNBD Disc", "SODTV DEL", "FUNTV TV Audio", "SODBD USE"] {
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
