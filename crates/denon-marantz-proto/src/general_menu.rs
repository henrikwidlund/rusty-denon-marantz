//! The general menu of the web interface: the language, the firmware update
//! settings, the sending of usage data and what switches each trigger
//! output.
//!
//! The pages are `/ajax/general/get_config?type=N` on port 11080 and do not
//! send changes. The meaning of the numbers is the one that the web
//! interface shows.

use crate::error::Error;
use crate::input_pages::source_for_index;
use crate::speaker_tables::{number, root};
use crate::values::Trigger;
use crate::xml::Node;

/// Most items of a trigger page that are kept.
const MAX_ITEMS: usize = 64;

code_enum! {
    /// The language of the menu on the screen. Which languages a receiver has
    /// depends on its region.
    pub enum Language {
        English = "1", "English";
        German = "2", "German";
        French = "3", "French";
        Italian = "4", "Italian";
        Spanish = "5", "Spanish";
        Dutch = "6", "Dutch";
        Swedish = "7", "Swedish";
        Japanese = "8", "Japanese";
        ChineseSimplified = "9", "Chinese (Simplified)";
        Polish = "10", "Polish";
        Russian = "11", "Russian";
    }
}

/// What a trigger item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TriggerItemKind {
    /// A zone: `0` is the main zone, `1` zone 2 and `2` zone 3. The output is
    /// on while the zone is on.
    Zone,
    /// An input source, by its index on the web interface. The output is on
    /// while the source is selected.
    Source,
    /// An HDMI monitor output: `0` is monitor 1 and `1` is monitor 2.
    HdmiOutput,
}

/// One thing that switches a trigger output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerItem {
    /// What the item is.
    pub kind: TriggerItemKind,
    /// The number of the zone, the index of the source or the number of the
    /// monitor.
    pub index: u8,
    /// Telnet name of the source, for a source that is known.
    pub source: Option<&'static str>,
    /// The item switches the output.
    pub enabled: bool,
}

/// What switches one trigger output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerAssignment {
    /// The trigger output.
    pub trigger: Trigger,
    /// The zones, sources and monitors, with a flag each.
    pub items: Vec<TriggerItem>,
}

impl TriggerAssignment {
    /// The items that switch the output.
    pub fn enabled(&self) -> impl Iterator<Item = &TriggerItem> {
        self.items.iter().filter(|item| item.enabled)
    }
}

/// The pages of the general menu that are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum GeneralPage {
    /// The language.
    Language,
    /// The firmware update settings.
    Firmware,
    /// The sending of usage data.
    UsageData,
    /// What switches a trigger output.
    Trigger(Trigger),
}

impl GeneralPage {
    /// All pages, in the order that they are read.
    pub const ALL: [Self; 6] = [
        Self::Language,
        Self::Firmware,
        Self::UsageData,
        Self::Trigger(Trigger::One),
        Self::Trigger(Trigger::Two),
        Self::Trigger(Trigger::Three),
    ];

    /// Path of the page.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::Language => "/ajax/general/get_config?type=2",
            Self::Firmware => "/ajax/general/get_config?type=11",
            Self::UsageData => "/ajax/general/get_config?type=14",
            Self::Trigger(Trigger::One) => "/ajax/general/get_config?type=8",
            Self::Trigger(Trigger::Two) => "/ajax/general/get_config?type=9",
            Self::Trigger(Trigger::Three) => "/ajax/general/get_config?type=20",
        }
    }

    const fn root_name(self) -> &'static str {
        match self {
            Self::Language => "Language",
            Self::Firmware => "Firmware",
            Self::UsageData => "UsageData",
            Self::Trigger(_) => "TriggerOut",
        }
    }

    /// Reads the page.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML, is an HTML page, is not
    /// the page that was asked for, or has no value that is known. A page
    /// without content must not replace what the state has.
    pub fn parse(self, body: &[u8]) -> Result<GeneralPart, Error> {
        let root = root(body)?;
        if !root.is(self.root_name()) {
            return Err(Error::UnexpectedDocument {
                expected: self.root_name(),
            });
        }
        let node = root.node();
        let part = match self {
            Self::Language => GeneralPart::Language(
                node.text()
                    .and_then(|text| Language::from_code(text.trim())),
            ),
            Self::Firmware => {
                let auto = node.child("AutoUpdate").map(|auto| auto.node());
                GeneralPart::Firmware {
                    auto_update: auto.and_then(|auto| on_off(auto, "Enable")),
                    time_zone: auto
                        .and_then(|auto| number::<u8>(auto, "TimeZone"))
                        .filter(|zone| (1..=43).contains(zone)),
                }
            }
            Self::UsageData => GeneralPart::UsageData(on_off(node, "Send")),
            Self::Trigger(trigger) => GeneralPart::Trigger(TriggerAssignment {
                trigger,
                items: trigger_items(node),
            }),
        };
        if part.has_content() {
            Ok(part)
        } else {
            Err(Error::UnexpectedDocument {
                expected: "a page with a known value",
            })
        }
    }
}

/// What a page of the general menu tells.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum GeneralPart {
    /// The language.
    Language(Option<Language>),
    /// The firmware update settings.
    Firmware {
        /// Firmware updates are installed by the receiver on its own.
        auto_update: Option<bool>,
        /// The time zone for the automatic update, as the number of the page.
        /// `1` is the time of the device. `2` to `43` are zones from 14 hours
        /// to -12 hours, for example `28` is +1 hour and `29` is 0.
        time_zone: Option<u8>,
    },
    /// The sending of usage data.
    UsageData(Option<bool>),
    /// What switches a trigger output.
    Trigger(TriggerAssignment),
}

impl GeneralPart {
    /// Returns true if the page gave at least one known value.
    const fn has_content(&self) -> bool {
        match self {
            Self::Language(value) => value.is_some(),
            Self::Firmware {
                auto_update,
                time_zone,
            } => auto_update.is_some() || time_zone.is_some(),
            Self::UsageData(value) => value.is_some(),
            Self::Trigger(assignment) => !assignment.items.is_empty(),
        }
    }
}

/// The general settings as the receiver told them. A value is `None` until
/// its page was read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GeneralSettings {
    /// The language of the menu.
    pub language: Option<Language>,
    /// Firmware updates are installed by the receiver on its own.
    pub auto_update: Option<bool>,
    /// The time zone for the automatic update, see [`GeneralPart::Firmware`].
    pub time_zone: Option<u8>,
    /// The receiver sends usage data.
    pub usage_data: Option<bool>,
    /// What switches each trigger output that was read.
    pub triggers: Vec<TriggerAssignment>,
}

impl GeneralSettings {
    /// What switches a trigger output, if it was read.
    #[must_use]
    pub fn trigger(&self, trigger: Trigger) -> Option<&TriggerAssignment> {
        self.triggers.iter().find(|known| known.trigger == trigger)
    }

    /// Stores what a page told. Returns true if something was different.
    pub(crate) fn apply(&mut self, part: GeneralPart) -> bool {
        fn put<T: PartialEq>(slot: &mut T, value: T) -> bool {
            if *slot == value {
                false
            } else {
                *slot = value;
                true
            }
        }
        match part {
            GeneralPart::Language(value) => put(&mut self.language, value),
            GeneralPart::Firmware {
                auto_update,
                time_zone,
            } => {
                // A page that lacks one value keeps the value that is known.
                let mut changed = false;
                if auto_update.is_some() {
                    changed |= put(&mut self.auto_update, auto_update);
                }
                if time_zone.is_some() {
                    changed |= put(&mut self.time_zone, time_zone);
                }
                changed
            }
            GeneralPart::UsageData(value) => put(&mut self.usage_data, value),
            GeneralPart::Trigger(new) => {
                if let Some(current) = self.triggers.iter_mut().find(|k| k.trigger == new.trigger) {
                    put(current, new)
                } else {
                    self.triggers.push(new);
                    true
                }
            }
        }
    }
}

/// `1` is on and `2` is off.
fn on_off(node: Node<'_>, name: &str) -> Option<bool> {
    match number::<u8>(node, name)? {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

fn trigger_items(node: Node<'_>) -> Vec<TriggerItem> {
    let mut items: Vec<TriggerItem> = Vec::new();
    for item in node.children().filter(|c| c.is("Item")) {
        if items.len() == MAX_ITEMS {
            break;
        }
        let attribute = |name: &str| {
            item.attribute(name)
                .and_then(|value| value.trim().parse::<u8>().ok())
        };
        let (Some(group), Some(index), Some(enabled)) = (
            attribute("group"),
            attribute("index"),
            on_off(item.node(), "Enable"),
        ) else {
            continue;
        };
        let kind = match group {
            1 => TriggerItemKind::Zone,
            2 => TriggerItemKind::Source,
            3 => TriggerItemKind::HdmiOutput,
            _ => continue,
        };
        // An item is listed once, and the first entry counts.
        if items
            .iter()
            .any(|known| known.kind == kind && known.index == index)
        {
            continue;
        }
        items.push(TriggerItem {
            kind,
            index,
            source: (kind == TriggerItemKind::Source)
                .then(|| source_for_index(index))
                .flatten(),
            enabled,
        });
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(page: GeneralPage, body: &[u8]) -> GeneralPart {
        page.parse(body).unwrap()
    }

    #[test]
    fn simple_pages_of_the_a10h() {
        assert_eq!(
            part(
                GeneralPage::Language,
                include_bytes!("../tests/data/xml/AVC-A10H-web-general-language.xml")
            ),
            GeneralPart::Language(Some(Language::English))
        );
        assert_eq!(
            part(
                GeneralPage::Firmware,
                include_bytes!("../tests/data/xml/AVC-A10H-web-general-firmware.xml")
            ),
            GeneralPart::Firmware {
                auto_update: Some(false),
                time_zone: Some(28),
            }
        );
        assert_eq!(
            part(
                GeneralPage::UsageData,
                include_bytes!("../tests/data/xml/AVC-A10H-web-general-usage-data.xml")
            ),
            GeneralPart::UsageData(Some(false))
        );
    }

    #[test]
    fn trigger_pages_tell_what_switches_each_output() {
        let GeneralPart::Trigger(one) = part(
            GeneralPage::Trigger(Trigger::One),
            include_bytes!("../tests/data/xml/AVC-A10H-web-general-trigger-a.xml"),
        ) else {
            panic!("not a trigger page");
        };
        assert_eq!(one.trigger, Trigger::One);
        // Only one source switches the first output.
        let enabled: Vec<_> = one.enabled().collect();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].kind, TriggerItemKind::Source);
        assert_eq!(enabled[0].source, Some("BD"));
        assert!(
            one.items
                .iter()
                .any(|i| i.kind == TriggerItemKind::HdmiOutput)
        );

        let GeneralPart::Trigger(two) = part(
            GeneralPage::Trigger(Trigger::Two),
            include_bytes!("../tests/data/xml/AVC-A10H-web-general-trigger-b.xml"),
        ) else {
            panic!("not a trigger page");
        };
        // The second output follows the main zone, for any source.
        let zone = |index: u8| {
            two.items
                .iter()
                .find(|item| item.kind == TriggerItemKind::Zone && item.index == index)
                .map(|item| item.enabled)
        };
        assert_eq!(
            (zone(0), zone(1), zone(2)),
            (Some(true), Some(false), Some(false))
        );
        assert!(
            two.enabled()
                .any(|item| item.kind == TriggerItemKind::Source)
        );
    }

    #[test]
    fn another_page_and_html_are_errors() {
        assert!(GeneralPage::Language.parse(b"<UsageData/>").is_err());
        assert!(
            GeneralPage::Firmware
                .parse(b"<html><body>no</body></html>")
                .is_err()
        );
    }

    #[test]
    fn a_page_without_a_known_value_is_an_error() {
        // The right root, but nothing usable: it must not clear what is known.
        for (kind, body) in [
            (GeneralPage::Language, &b"<Language>99</Language>"[..]),
            (GeneralPage::Language, b"<Language/>"),
            (GeneralPage::Firmware, b"<Firmware/>"),
            (
                GeneralPage::Firmware,
                b"<Firmware><AutoUpdate><TimeZone>99</TimeZone></AutoUpdate></Firmware>",
            ),
            (GeneralPage::UsageData, b"<UsageData/>"),
            (GeneralPage::Trigger(Trigger::One), b"<TriggerOut/>"),
            (
                GeneralPage::Trigger(Trigger::One),
                b"<TriggerOut><Item group=\"9\" index=\"0\"><Enable>1</Enable></Item></TriggerOut>",
            ),
        ] {
            assert!(kind.parse(body).is_err(), "{kind:?}");
        }
    }

    #[test]
    fn a_trigger_item_is_listed_once() {
        let GeneralPart::Trigger(trigger) = part(
            GeneralPage::Trigger(Trigger::Two),
            b"<TriggerOut><Item group=\"1\" index=\"0\"><Enable>1</Enable></Item>\
              <Item group=\"1\" index=\"0\"><Enable>2</Enable></Item></TriggerOut>",
        ) else {
            panic!("not a trigger page");
        };
        assert_eq!(trigger.items.len(), 1);
        assert!(trigger.items[0].enabled);
    }

    #[test]
    fn the_same_firmware_page_again_changes_nothing() {
        let mut settings = GeneralSettings::default();
        let firmware = || {
            part(
                GeneralPage::Firmware,
                include_bytes!("../tests/data/xml/AVC-A10H-web-general-firmware.xml"),
            )
        };
        assert!(settings.apply(firmware()));
        assert!(!settings.apply(firmware()));
        // One value of the page is enough for a change.
        assert!(settings.apply(GeneralPart::Firmware {
            auto_update: Some(true),
            time_zone: Some(28),
        }));
    }

    #[test]
    fn a_page_without_a_value_keeps_the_known_value() {
        let mut settings = GeneralSettings::default();
        assert!(settings.apply(GeneralPart::Firmware {
            auto_update: Some(true),
            time_zone: Some(29),
        }));
        assert!(!settings.apply(GeneralPart::Firmware {
            auto_update: Some(true),
            time_zone: None,
        }));
        assert_eq!(settings.time_zone, Some(29));
        assert!(settings.apply(GeneralPart::Firmware {
            auto_update: None,
            time_zone: Some(28),
        }));
        assert_eq!(settings.auto_update, Some(true));
    }

    #[test]
    fn settings_keep_each_trigger_once() {
        let mut settings = GeneralSettings::default();
        let page = |enabled: &str| {
            let body = format!(
                "<TriggerOut index=\"2\"><Item group=\"1\" index=\"0\"><Enable>{enabled}</Enable></Item></TriggerOut>"
            );
            part(GeneralPage::Trigger(Trigger::One), body.as_bytes())
        };
        assert!(settings.apply(page("1")));
        assert!(!settings.apply(page("1")));
        assert!(settings.apply(page("2")));
        assert_eq!(settings.triggers.len(), 1);
        assert_eq!(settings.trigger(Trigger::One).unwrap().enabled().count(), 0);
        assert!(settings.trigger(Trigger::Two).is_none());
    }
}
