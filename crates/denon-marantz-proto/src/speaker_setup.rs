//! Speaker crossovers, speaker configuration and graphic EQ that a receiver
//! reports on telnet as `SS` events. The spec does not describe them. The
//! texts come from the answers of an AVC-A10H. The web interface add the
//! distances, the bass settings and the crossover choices.

use std::collections::BTreeMap;

use crate::channel::Channel;
use crate::speaker_tables::{BassSettings, SpeakerTable};

/// Most entries of each map that are kept. A faulty receiver cannot make the
/// maps grow without a limit.
const MAX_ENTRIES: usize = 64;

code_enum! {
    /// A group of speakers that share a crossover. The codes are the ones that
    /// the receiver uses, the labels are a reading of them.
    pub enum SpeakerGroup {
        Front = "FRO", "Front";
        Center = "CEN", "Center";
        Surround = "SUA", "Surround";
        SurroundBack = "SBK", "Surround back";
        FrontHeight = "FRH", "Front height";
        TopFront = "TFR", "Top front";
        TopMiddle = "TPM", "Top middle";
        FrontDolby = "FRD", "Front Dolby";
        SurroundDolby = "SUD", "Surround Dolby";
        FrontWide = "FRW", "Front wide";
        TopRear = "TPR", "Top rear";
        RearHeight = "RHE", "Rear height";
        BackDolby = "BKD", "Back Dolby";
        SurroundHeight = "SHE", "Surround height";
        TopSurround = "TPS", "Top surround";
        CenterHeight = "CHE", "Center height";
    }
}

/// Crossover of a group of speakers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Crossover {
    /// Full range, no crossover.
    Full,
    /// Crossover frequency in Hz.
    Hz(u16),
}

impl Crossover {
    /// A value of the web interface: `0` is full range, other values are
    /// Hz. `None` for a value below 0 or above 65535.
    #[must_use]
    pub fn from_page_value(value: i32) -> Option<Self> {
        match u16::try_from(value).ok()? {
            0 => Some(Self::Full),
            hz => Some(Self::Hz(hz)),
        }
    }

    /// Parses the value of an event: `FUL` or three digits, for example `080`.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        if code == "FUL" {
            return Some(Self::Full);
        }
        if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        code.parse().ok().map(Self::Hz)
    }
}

/// What the receiver reports about the speakers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpeakerSetup {
    /// Crossover of each group of speakers, in the order of the report.
    pub crossovers: Vec<(SpeakerGroup, Crossover)>,
    /// Speaker configuration as the receiver sends it, for example `FRO` is
    /// `YES`. A value like `2SP` is kept under the code and `-SP`, for
    /// example `SBK-SP`. The meaning of the codes is not documented.
    pub config: BTreeMap<String, String>,
    /// Graphic EQ values of each group, for example `FRO` is
    /// `500:500:500:500:500:500:500:500:500`. The unit is not documented.
    pub graphic_eq: BTreeMap<String, String>,
    /// The crossovers that the receiver offers. Empty until the web interface
    /// page has been read.
    pub crossover_choices: Vec<Crossover>,
    /// Distances of the speakers from the web interface page. `None` until it
    /// has been read.
    pub distances: Option<SpeakerTable<Channel>>,
    /// Number of subwoofers, from 1 to 4. `None` until the web interface page
    /// has been read.
    pub subwoofer_count: Option<u8>,
    /// Bass settings from the web interface page. `None` until it has been
    /// read.
    pub bass: Option<BassSettings>,
}

impl SpeakerSetup {
    /// Applies an `SS` event. Returns `None` if the event is not about the
    /// speaker setup, and otherwise true if the setup changed. The end lines
    /// of the lists are ignored.
    pub(crate) fn apply(&mut self, parameter: &str) -> Option<bool> {
        if let Some(rest) = parameter.strip_prefix("CFR") {
            return self.apply_crossover(rest);
        }
        if let Some(rest) = parameter.strip_prefix("SPC") {
            let (code, value) = split_code(rest)?;
            let value = value.trim();
            if value.is_empty()
                || value.len() > 8
                || !value.bytes().all(|b| b.is_ascii_alphanumeric())
            {
                return None;
            }
            let has_count = value.len() > 2 && value.ends_with("SP");
            let mut key = String::with_capacity(code.len() + 3);
            key.push_str(code);
            if has_count {
                key.push_str("-SP");
            }
            return Some(insert(&mut self.config, &key, value));
        }
        if let Some(rest) = parameter.strip_prefix("AEQLRS") {
            if rest.trim() == "END" {
                return Some(false);
            }
            let (code, value) = split_code(rest)?;
            let value = value.trim();
            if value.is_empty()
                || value.len() > 64
                || !value.bytes().all(|b| b.is_ascii_digit() || b == b':')
            {
                return None;
            }
            return Some(insert(&mut self.graphic_eq, code, value));
        }
        None
    }

    fn apply_crossover(&mut self, rest: &str) -> Option<bool> {
        if rest.trim() == "END" {
            return Some(false);
        }
        let (code, value) = rest.split_once(' ')?;
        let group = SpeakerGroup::from_code(code)?;
        let crossover = Crossover::from_code(value.trim())?;
        Some(self.set_crossover(group, crossover))
    }

    /// Sets the crossover of a group. Returns true if it changed.
    fn set_crossover(&mut self, group: SpeakerGroup, crossover: Crossover) -> bool {
        if let Some((_, current)) = self
            .crossovers
            .iter_mut()
            .find(|(known, _)| *known == group)
        {
            return std::mem::replace(current, crossover) != crossover;
        }
        self.crossovers.push((group, crossover));
        true
    }

    /// Stores the crossovers and the choices of the crossovers page of the
    /// web interface. Returns true if anything changed.
    pub(crate) fn apply_crossover_table(&mut self, table: &SpeakerTable<SpeakerGroup>) -> bool {
        let mut changed = false;
        for entry in &table.entries {
            if let Some(crossover) = Crossover::from_page_value(entry.value) {
                changed |= self.set_crossover(entry.key, crossover);
            }
        }
        let choices: Vec<_> = table
            .choices
            .iter()
            .filter_map(|&hz| Crossover::from_page_value(hz))
            .collect();
        if self.crossover_choices != choices {
            self.crossover_choices = choices;
            changed = true;
        }
        changed
    }

    /// Stores the distances of the web interface page. Returns true if anything
    /// changed.
    pub(crate) fn apply_distances(&mut self, table: SpeakerTable<Channel>) -> bool {
        let count = table.subwoofer_count;
        let mut changed = self.subwoofer_count != count;
        self.subwoofer_count = count;
        let table = Some(table);
        if self.distances != table {
            self.distances = table;
            changed = true;
        }
        changed
    }

    /// Stores the bass page of the web interface. Returns true if it changed.
    pub(crate) fn apply_bass(&mut self, bass: BassSettings) -> bool {
        let bass = Some(bass);
        let changed = self.bass != bass;
        self.bass = bass;
        changed
    }
}

/// Splits the code of three letters from the value, for example `FRO YES`.
fn split_code(text: &str) -> Option<(&str, &str)> {
    let (code, value) = text.split_once(' ')?;
    (code.len() == 3 && code.bytes().all(|b| b.is_ascii_alphanumeric())).then_some((code, value))
}

/// Sets a value in a map. Returns true if the map changed.
fn insert(map: &mut BTreeMap<String, String>, key: &str, value: &str) -> bool {
    if let Some(current) = map.get_mut(key) {
        if current == value {
            return false;
        }
        current.clear();
        current.push_str(value);
        return true;
    }
    if map.len() >= MAX_ENTRIES {
        return false;
    }
    map.insert(key.to_owned(), value.to_owned());
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn web_pages_join_the_telnet_values() {
        let mut setup = SpeakerSetup::default();
        setup.apply("CFRCEN 080");
        let table = SpeakerTable::parse_crossovers(
            br#"<Crossovers><List><Speaker index="0">0</Speaker><Speaker index="1">80</Speaker>
            </List><SelectableValue><List><Item>0</Item><Item>40</Item></List>
            </SelectableValue></Crossovers>"#,
        )
        .unwrap();
        assert!(setup.apply_crossover_table(&table));
        assert!(!setup.apply_crossover_table(&table));
        assert_eq!(
            setup.crossovers,
            [
                (SpeakerGroup::Center, Crossover::Hz(80)),
                (SpeakerGroup::Front, Crossover::Full)
            ]
        );
        assert_eq!(
            setup.crossover_choices,
            [Crossover::Full, Crossover::Hz(40)]
        );
        // Telnet goes on to change the same values.
        assert_eq!(setup.apply("CFRFRO 100"), Some(true));
        assert_eq!(
            setup.crossovers[1],
            (SpeakerGroup::Front, Crossover::Hz(100))
        );
        assert_eq!(Crossover::from_page_value(-1), None);
        assert_eq!(Crossover::from_page_value(70_000), None);
    }

    #[test]
    fn crossovers_follow_events() {
        let mut setup = SpeakerSetup::default();
        assert_eq!(setup.apply("CFRFRO FUL"), Some(true));
        assert_eq!(setup.apply("CFRFRO FUL"), Some(false));
        assert_eq!(setup.apply("CFRCEN 080"), Some(true));
        assert_eq!(setup.apply("CFRCEN 120"), Some(true));
        assert_eq!(setup.apply("CFR END"), Some(false));
        assert_eq!(
            setup.crossovers,
            [
                (SpeakerGroup::Front, Crossover::Full),
                (SpeakerGroup::Center, Crossover::Hz(120))
            ]
        );
        for bad in [
            "CFRXXX 080",
            "CFRFRO",
            "CFRFRO 8",
            "CFRFRO abc",
            "CFRFRO 0800",
        ] {
            assert_eq!(setup.apply(bad), None, "{bad}");
        }
        assert_eq!(setup.crossovers.len(), 2);
    }

    #[test]
    fn speaker_config_follows_events() {
        let mut setup = SpeakerSetup::default();
        assert_eq!(setup.apply("SPCFRO YES"), Some(true));
        assert_eq!(setup.apply("SPCSBK YES"), Some(true));
        assert_eq!(setup.apply("SPCSBK 2SP"), Some(true));
        assert_eq!(setup.apply("SPCSBK 2SP"), Some(false));
        assert_eq!(setup.apply("SPCSWM STD"), Some(true));
        assert_eq!(setup.config["SBK"], "YES");
        assert_eq!(setup.config["SBK-SP"], "2SP");
        assert_eq!(setup.config["SWM"], "STD");
        assert_eq!(setup.apply("SPCFRO NO"), Some(true));
        assert_eq!(setup.config["FRO"], "NO");
        for bad in ["SPCFRO", "SPCFR YES", "SPCFRO Y E S", "SPCFRO TOOLONGVALUE"] {
            assert_eq!(setup.apply(bad), None, "{bad}");
        }
    }

    #[test]
    fn graphic_eq_follows_events() {
        let mut setup = SpeakerSetup::default();
        let flat = "500:500:500:500:500:500:500:500:500";
        assert_eq!(setup.apply(&format!("AEQLRSFRO {flat}")), Some(true));
        assert_eq!(setup.apply(&format!("AEQLRSFRO {flat}")), Some(false));
        assert_eq!(setup.apply("AEQLRSEND"), Some(false));
        assert_eq!(setup.graphic_eq["FRO"], flat);
        for bad in ["AEQLRSFRO", "AEQLRSFRO abc", "AEQLRSFRO 500:x"] {
            assert_eq!(setup.apply(bad), None, "{bad}");
        }
        assert_eq!(setup.apply("TTRLEV 50"), None);
    }

    #[test]
    fn maps_stay_small() {
        let mut setup = SpeakerSetup::default();
        for first in b'A'..=b'Z' {
            for second in b'A'..=b'H' {
                let code = [char::from(first), char::from(second), 'X'];
                let code: String = code.iter().collect();
                setup.apply(&format!("SPC{code} YES"));
            }
        }
        assert_eq!(setup.config.len(), MAX_ENTRIES);
    }
}
