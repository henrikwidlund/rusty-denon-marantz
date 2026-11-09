//! Which settings the receiver offers and lets the user change right now.
//!
//! The pages `/ajax/<group>/get_config?type=1` of the web interface list the
//! settings of a group, for example the speaker settings. Each setting has a number
//! that says if the receiver offers it and can be used now, and, for a setting
//! that is offered but blocked, a code for the reason.

use crate::error::Error;
use crate::speaker_tables::root;

/// Most settings of a group that are kept.
const MAX_ITEMS: usize = 64;

/// A group of settings, as the web interface orders them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SettingsGroup {
    /// The audio settings.
    Audio,
    /// The video settings.
    Video,
    /// The input settings.
    Inputs,
    /// The speaker settings.
    Speakers,
    /// The network settings. Only which of them are offered is read, not
    /// their values.
    Network,
    /// The general settings.
    General,
    /// The options: settings that are quick to reach during playback.
    Option,
}

impl SettingsGroup {
    /// All groups.
    pub const ALL: [Self; 7] = [
        Self::Audio,
        Self::Video,
        Self::Inputs,
        Self::Speakers,
        Self::Network,
        Self::General,
        Self::Option,
    ];

    /// Path of the page of the group.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Audio => "/ajax/audio/get_config?type=1",
            Self::Video => "/ajax/video/get_config?type=1",
            Self::Inputs => "/ajax/inputs/get_config?type=1",
            Self::Speakers => "/ajax/speakers/get_config?type=1",
            Self::Network => "/ajax/network/get_config?type=1",
            Self::General => "/ajax/general/get_config?type=1",
            Self::Option => "/ajax/option/get_config?type=1",
        }
    }
}

/// If the receiver offers a setting and lets the user change it now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Availability {
    /// The receiver does not offer the setting now.
    NotOffered,
    /// The receiver offers the setting, but it cannot be changed now.
    Blocked,
    /// The item can be used.
    Available,
}

impl Availability {
    /// The availability for the number of the page: `1` is not offered, `2`
    /// is blocked, `3` and `4` are available.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::NotOffered),
            2 => Some(Self::Blocked),
            3 | 4 => Some(Self::Available),
            _ => None,
        }
    }
}

/// A setting and if it can be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingAvailability {
    /// Name of the setting on the page, for example `Crossovers`.
    pub name: String,
    /// If the receiver offers the setting and lets the user change it now.
    pub availability: Availability,
    /// Why a blocked setting cannot be changed, as the code of the page.
    /// `None` for a setting that is not blocked, or without a code. The
    /// meaning of the code is different for each setting, and the receiver
    /// gives the same code for different causes. `3` for `Crossovers` means that a Dirac ART
    /// filter is active.
    pub reason_code: Option<u8>,
}

/// The settings of a group and if they can be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupAvailability {
    /// The group.
    pub group: SettingsGroup,
    /// The settings, in the order of the page.
    pub items: Vec<SettingAvailability>,
}

impl GroupAvailability {
    /// Reads the page of a group. A setting without a known number is left
    /// out.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML or is an HTML page.
    pub fn parse(group: SettingsGroup, body: &[u8]) -> Result<Self, Error> {
        let node = root(body)?.node();
        let mut items = Vec::new();
        for item in node.children() {
            if items.len() == MAX_ITEMS {
                break;
            }
            let Some(availability) = item
                .node()
                .text()
                .and_then(|text| text.trim().parse::<u8>().ok())
                .and_then(Availability::from_code)
            else {
                continue;
            };
            items.push(SettingAvailability {
                name: item.name(),
                availability,
                reason_code: item
                    .attribute("gray")
                    .and_then(|gray| gray.trim().parse().ok())
                    .filter(|&code| code != 0),
            });
        }
        Ok(Self { group, items })
    }

    /// The setting with this name.
    #[must_use]
    pub fn item(&self, name: &str) -> Option<&SettingAvailability> {
        self.items.iter().find(|item| item.name == name)
    }
}

/// Which settings the receiver offers and lets the user change now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsAvailability {
    groups: Vec<GroupAvailability>,
}

impl SettingsAvailability {
    /// The settings of a group, if the receiver gave them.
    #[must_use]
    pub fn group(&self, group: SettingsGroup) -> Option<&GroupAvailability> {
        self.groups.iter().find(|known| known.group == group)
    }

    /// A setting of a group, if the receiver gave it.
    #[must_use]
    pub fn item(&self, group: SettingsGroup, name: &str) -> Option<&SettingAvailability> {
        self.group(group)?.item(name)
    }

    /// Stores a group. Returns true if it is new or different.
    pub(crate) fn set(&mut self, new: GroupAvailability) -> bool {
        match self
            .groups
            .iter_mut()
            .find(|known| known.group == new.group)
        {
            Some(current) if *current == new => false,
            Some(current) => {
                *current = new;
                true
            }
            None => {
                self.groups.push(new);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEAKERS: &[u8] =
        include_bytes!("../tests/data/xml/AVC-A10H-web-availability-speakers.xml");
    const AUDIO: &[u8] = include_bytes!("../tests/data/xml/AVC-A10H-web-availability-audio.xml");
    const OPTION: &[u8] = include_bytes!("../tests/data/xml/AVC-A10H-web-availability-option.xml");
    const NETWORK: &[u8] =
        include_bytes!("../tests/data/xml/AVC-A10H-web-availability-network.xml");

    #[test]
    fn speaker_settings_of_the_a10h() {
        let group = GroupAvailability::parse(SettingsGroup::Speakers, SPEAKERS).unwrap();
        assert_eq!(group.items.len(), 20);
        // Dirac ART locks the crossovers.
        let crossovers = group.item("Crossovers").unwrap();
        assert_eq!(crossovers.availability, Availability::Blocked);
        assert_eq!(crossovers.reason_code, Some(3));
        let levels = group.item("Levels").unwrap();
        assert_eq!(levels.availability, Availability::Available);
        // A code of 0 means no reason.
        assert_eq!(levels.reason_code, None);
        assert_eq!(
            group.item("LFEDistribution").unwrap().availability,
            Availability::Blocked
        );
        assert_eq!(
            group.item("AmpAssign").unwrap().availability,
            Availability::NotOffered
        );
        assert_eq!(
            group.item("AdvancedSetup").unwrap().availability,
            Availability::Available
        );
        assert!(group.item("Nothing").is_none());
    }

    #[test]
    fn audio_and_option_settings_of_the_a10h() {
        let audio = GroupAvailability::parse(SettingsGroup::Audio, AUDIO).unwrap();
        assert_eq!(
            audio.item("Restorer").unwrap().availability,
            Availability::Blocked
        );
        assert_eq!(
            audio.item("Volume").unwrap().availability,
            Availability::Available
        );
        let option = GroupAvailability::parse(SettingsGroup::Option, OPTION).unwrap();
        assert_eq!(
            option.item("DiracLive").unwrap().availability,
            Availability::Available
        );
        assert_eq!(
            option.item("Tone").unwrap().availability,
            Availability::NotOffered
        );
    }

    #[test]
    fn network_settings_have_only_flags() {
        let group = GroupAvailability::parse(SettingsGroup::Network, NETWORK).unwrap();
        assert_eq!(group.items.len(), 12);
        assert_eq!(
            group.item("Connection").unwrap().availability,
            Availability::NotOffered
        );
        assert_eq!(
            group.item("AirPlay").unwrap().availability,
            Availability::Available
        );
        assert_eq!(
            SettingsGroup::Network.path(),
            "/ajax/network/get_config?type=1"
        );
        assert_eq!(SettingsGroup::ALL.len(), 7);
    }

    #[test]
    fn unknown_numbers_and_bad_bodies() {
        let group = GroupAvailability::parse(
            SettingsGroup::General,
            br#"<listGroupAvailability><A>9</A><B gray="2">2</B><C>x</C><D/></listGroupAvailability>"#,
        )
        .unwrap();
        let names: Vec<_> = group.items.iter().map(|item| item.name.as_str()).collect();
        assert_eq!(names, ["B"]);
        assert_eq!(group.items[0].reason_code, Some(2));
        assert!(GroupAvailability::parse(SettingsGroup::Audio, b"<html/>").is_err());
        assert!(GroupAvailability::parse(SettingsGroup::Audio, b"").is_err());
        assert_eq!(Availability::from_code(4), Some(Availability::Available));
        assert_eq!(Availability::from_code(0), None);
        assert_eq!(
            SettingsGroup::Speakers.path(),
            "/ajax/speakers/get_config?type=1"
        );
    }

    #[test]
    fn groups_are_stored_by_group() {
        let mut groups = SettingsAvailability::default();
        let speakers = GroupAvailability::parse(SettingsGroup::Speakers, SPEAKERS).unwrap();
        assert!(groups.set(speakers.clone()));
        assert!(!groups.set(speakers));
        assert!(groups.group(SettingsGroup::Audio).is_none());
        let changed = GroupAvailability::parse(
            SettingsGroup::Speakers,
            b"<listGroupAvailability><Crossovers>3</Crossovers></listGroupAvailability>",
        )
        .unwrap();
        assert!(groups.set(changed));
        assert_eq!(
            groups
                .item(SettingsGroup::Speakers, "Crossovers")
                .unwrap()
                .availability,
            Availability::Available
        );
        assert_eq!(
            groups.group(SettingsGroup::Speakers).unwrap().items.len(),
            1
        );
    }
}
