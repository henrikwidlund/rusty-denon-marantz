//! The sound modes that a receiver offers, in groups. Telnet reports them.

use std::mem;

use crate::sound_mode::SoundModeGroup;

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

/// The names of the list and the group whose code selects the mode. The names
/// are the ones that receivers write in the list. Some receivers write the
/// same mode in another way, so a name that is not here has no code.
const LIST_NAMES: [(&str, SoundModeGroup); 22] = [
    ("Stereo", SoundModeGroup::Stereo),
    ("Dolby Surround", SoundModeGroup::DolbyDigital),
    ("Dolby Digital", SoundModeGroup::DolbyDigital),
    ("DTS Neural:X", SoundModeGroup::DtsSurround),
    ("DTS Surround", SoundModeGroup::DtsSurround),
    ("AURO-3D", SoundModeGroup::Auro3d),
    ("AURO Surround", SoundModeGroup::Auro2dSurround),
    ("Multi Ch Stereo", SoundModeGroup::McHStereo),
    ("Multi Channel Stereo", SoundModeGroup::McHStereo),
    ("Mono Movie", SoundModeGroup::MonoMovie),
    ("Rock Arena", SoundModeGroup::RockArena),
    ("Jazz Club", SoundModeGroup::JazzClub),
    ("Matrix", SoundModeGroup::Matrix),
    ("Video Game", SoundModeGroup::VideoGame),
    ("Super Stadium", SoundModeGroup::SuperStadium),
    ("Classic Concert", SoundModeGroup::ClassicConcert),
    ("Wide Screen", SoundModeGroup::WideScreen),
    ("Virtual", SoundModeGroup::Virtual),
    ("All Zone Stereo", SoundModeGroup::AllZoneStereo),
    ("Direct", SoundModeGroup::Direct),
    ("Pure Direct", SoundModeGroup::PureDirect),
    ("Auto", SoundModeGroup::Auto),
];

impl SoundModeEntry {
    /// The group whose code selects this mode, if the library knows the name.
    #[must_use]
    pub fn group(&self) -> Option<SoundModeGroup> {
        LIST_NAMES
            .iter()
            .find(|(name, _)| {
                // Case and extra spaces do not matter.
                name.split_whitespace()
                    .map(str::to_ascii_lowercase)
                    .eq(self.name.split_whitespace().map(str::to_ascii_lowercase))
            })
            .map(|(_, group)| *group)
    }

    /// The code that selects this mode, for example `DOLBY DIGITAL` for
    /// `Dolby Surround`. `None` if the library does not know the name.
    ///
    /// The code does not say the category: when a name is in more than one
    /// category (Movie, Music and Game), the receiver chooses the one that
    /// fits the current mode. The receiver reports the active mode under its
    /// own raw name (for example `DOLBY AUDIO - DOLBY SURROUND`), which is
    /// not the code.
    #[must_use]
    pub fn code(&self) -> Option<&'static str> {
        self.group().map(SoundModeGroup::code)
    }
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
            // The same place again means that a new list has started, and the
            // lines of a list that was cut short are not part of it.
            if self
                .pending
                .iter()
                .any(|e| e.category == entry.category && e.index == entry.index)
            {
                self.pending.clear();
            }
            self.pending.push(entry);
        }
        Some(false)
    }
}

/// Reads `MOV020Dolby Surround`: a group, three digits and the name.
fn parse_entry(text: &str) -> Option<SoundModeEntry> {
    let category = SoundModeCategory::from_code(text.get(..3)?)?;
    let digits = text.get(3..6)?;
    if !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let index = digits.parse().ok()?;
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
    fn a_list_that_was_cut_short_leaves_nothing_in_the_next_one() {
        let mut list = SoundModeList::default();
        feed(&mut list, &["SMLALL MOV001Stereo", "SMLALL MOV002Matrix"]);
        feed(&mut list, &["SMLALL MOV001Stereo", "SMLALL END"]);
        assert_eq!(list.as_slice().len(), 1);
        assert_eq!(list.as_slice()[0].name, "Stereo");
    }

    #[test]
    fn the_same_index_in_two_groups_is_two_modes_and_signs_are_not_digits() {
        let mut list = SoundModeList::default();
        feed(
            &mut list,
            &[
                "SMLALL MOV011Stereo",
                "SMLALL MUS011Jazz Club",
                "SMLALL MOV+12Matrix",
                "SMLALL END",
            ],
        );
        let names: Vec<_> = list.as_slice().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["Stereo", "Jazz Club"]);
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

    fn entry(category: SoundModeCategory, name: &str) -> SoundModeEntry {
        SoundModeEntry {
            category,
            index: 1,
            name: name.to_owned(),
        }
    }

    #[test]
    fn the_modes_of_the_a10h_have_a_code() {
        use SoundModeCategory::{Game, Movie, Music, Pure};
        let list = [
            (Movie, "Stereo", "STEREO"),
            (Movie, "Dolby Surround", "DOLBY DIGITAL"),
            (Movie, "DTS Neural:X", "DTS SURROUND"),
            (Movie, "AURO-3D", "AURO3D"),
            (Movie, "AURO Surround", "AURO2DSURR"),
            (Movie, "Multi Ch Stereo", "MCH STEREO"),
            (Movie, "Mono Movie", "MONO MOVIE"),
            (Music, "Dolby Surround", "DOLBY DIGITAL"),
            (Music, "Rock Arena", "ROCK ARENA"),
            (Music, "Jazz Club", "JAZZ CLUB"),
            (Music, "Matrix", "MATRIX"),
            (Game, "Video Game", "VIDEO GAME"),
            (Pure, "Direct", "DIRECT"),
            (Pure, "Pure Direct", "PURE DIRECT"),
        ];
        for (category, name, code) in list {
            assert_eq!(entry(category, name).code(), Some(code), "{name}");
        }
    }

    #[test]
    fn case_and_spaces_in_a_name_do_not_matter_and_an_unknown_name_has_no_code() {
        let movie = SoundModeCategory::Movie;
        assert_eq!(
            entry(movie, "  dolby   SURROUND ").code(),
            Some("DOLBY DIGITAL")
        );
        assert_eq!(entry(movie, "Something New").code(), None);
        assert_eq!(entry(movie, "").code(), None);
        assert_eq!(entry(movie, "Stereo").group(), Some(SoundModeGroup::Stereo));
    }
}
