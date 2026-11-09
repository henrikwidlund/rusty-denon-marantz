//! The sound modes that a receiver offers, in groups. Telnet reports them.

use std::mem;

code_enum! {
    /// Group of a sound mode in the list of the receiver.
    pub enum SoundModeCategory {
        Movie = "MOV", "Movie";
        Music = "MUS", "Music";
        Game = "GAM", "Game";
        Pure = "PUR", "Pure";
    }
}

/// One sound mode that the receiver offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SoundModeEntry {
    /// Group of the mode.
    pub category: SoundModeCategory,
    /// Number of the mode in the list of the receiver.
    pub index: u16,
    /// Name of the mode, for example `Dolby Surround`. The same name can be
    /// in more than one group.
    pub name: String,
}

/// The sound modes of a receiver, in the order that the receiver reports them.
///
/// The receiver sends the whole list, then an end line. The list changes at
/// the end line, so a list that is cut short does not change it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SoundModeList {
    modes: Vec<SoundModeEntry>,
    pending: Vec<SoundModeEntry>,
}

impl SoundModeList {
    /// The sound modes, in the order that the receiver reports them.
    #[must_use]
    pub fn as_slice(&self) -> &[SoundModeEntry] {
        &self.modes
    }

    /// The sound modes of one group.
    pub fn in_category(
        &self,
        category: SoundModeCategory,
    ) -> impl Iterator<Item = &SoundModeEntry> {
        self.modes
            .iter()
            .filter(move |mode| mode.category == category)
    }

    /// Applies the text of an `OP` event, for example
    /// `SMLALL MOV020Dolby Surround` or `SMLALL END`.
    ///
    /// Returns `None` if the event is not about sound modes. Otherwise returns
    /// true if the list changed.
    pub(crate) fn apply_op(&mut self, parameter: &str) -> Option<bool> {
        let rest = parameter.strip_prefix("SMLALL ")?.trim_end();
        if rest == "END" {
            let list = mem::take(&mut self.pending);
            if list.is_empty() || list == self.modes {
                return Some(false);
            }
            self.modes = list;
            return Some(true);
        }
        if let Some(entry) = parse_entry(rest) {
            match self.pending.iter_mut().find(|e| e.index == entry.index) {
                Some(existing) => *existing = entry,
                None => self.pending.push(entry),
            }
        }
        Some(false)
    }
}

/// Reads `MOV020Dolby Surround`: a group, three digits and the name.
fn parse_entry(text: &str) -> Option<SoundModeEntry> {
    let category = SoundModeCategory::from_code(text.get(..3)?)?;
    let index = text.get(3..6)?.parse().ok()?;
    let name = text.get(6..)?.trim();
    (!name.is_empty()).then(|| SoundModeEntry {
        category,
        index,
        name: name.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(list: &mut SoundModeList, lines: &[&str]) -> Vec<Option<bool>> {
        lines.iter().map(|line| list.apply_op(line)).collect()
    }

    #[test]
    fn the_list_changes_at_the_end_line() {
        let mut list = SoundModeList::default();
        let results = feed(
            &mut list,
            &[
                "SMLALL MOV011Stereo",
                "SMLALL MUS080Stereo ",
                "SMLALL PUR250Pure Direct",
            ],
        );
        assert_eq!(results, [Some(false); 3]);
        assert_eq!(list.as_slice(), []);
        assert_eq!(list.apply_op("SMLALL END"), Some(true));
        let names: Vec<_> = list.as_slice().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Stereo", "Stereo", "Pure Direct"]);
        assert_eq!(list.as_slice()[1].category, SoundModeCategory::Music);
        assert_eq!(list.as_slice()[1].index, 80);
        assert_eq!(list.in_category(SoundModeCategory::Pure).count(), 1);
    }

    #[test]
    fn the_same_list_again_is_no_change() {
        let mut list = SoundModeList::default();
        feed(&mut list, &["SMLALL MOV011Stereo", "SMLALL END"]);
        let again = feed(&mut list, &["SMLALL MOV011Stereo", "SMLALL END"]);
        assert_eq!(again, [Some(false), Some(false)]);
    }

    #[test]
    fn a_new_list_replaces_the_old_one() {
        let mut list = SoundModeList::default();
        feed(
            &mut list,
            &[
                "SMLALL MOV011Stereo",
                "SMLALL MOV020Dolby Surround",
                "SMLALL END",
            ],
        );
        let results = feed(&mut list, &["SMLALL MOV011Stereo", "SMLALL END"]);
        assert_eq!(results, [Some(false), Some(true)]);
        assert_eq!(list.as_slice().len(), 1);
    }

    #[test]
    fn a_repeated_index_is_kept_once() {
        let mut list = SoundModeList::default();
        feed(
            &mut list,
            &["SMLALL MOV011Stereo", "SMLALL MOV011Stereo", "SMLALL END"],
        );
        assert_eq!(list.as_slice().len(), 1);
    }

    #[test]
    fn bad_lines_are_skipped() {
        let mut list = SoundModeList::default();
        let results = feed(
            &mut list,
            &[
                "SMLALL XXX011Stereo",
                "SMLALL MOV01",
                "SMLALL MOVabcStereo",
                "SMLALL MOV011   ",
                "SMLALL END",
            ],
        );
        assert_eq!(results, [Some(false); 5]);
        assert_eq!(list.as_slice(), []);
    }

    #[test]
    fn other_lines_are_not_sound_modes() {
        let mut list = SoundModeList::default();
        assert_eq!(list.apply_op("DLSSL1NAME Cinema"), None);
        assert_eq!(list.apply_op("SML 011Stereo"), None);
        assert_eq!(list.apply_op("STS 0000"), None);
    }
}
