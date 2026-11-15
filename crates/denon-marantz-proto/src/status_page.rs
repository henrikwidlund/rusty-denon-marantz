//! Status pages of receivers without `AppCommand`.
//!
//! Older receivers answer `/goform/formMainZone_MainZoneXmlStatus.xml` and
//! similar pages. Each value is in a `value` element, for example
//! `<Power><value>ON</value></Power>`. The values are the same kind as in
//! `AppCommand` answers, so they go to [`State::apply_field`](crate::State::apply_field).

use crate::appcommand::{Field, Leaf, Step, find};
use crate::error::Error;
use crate::xml::{Node, check_well_formed};
use crate::zone::Zone;

/// Page name of the second page. `concat!` needs a macro for a shared literal.
macro_rules! zone_page {
    () => {
        "/goform/formMainZone_MainZoneXml.xml"
    };
}

/// A checked status page. It borrows the body.
#[derive(Debug, Clone, Copy)]
pub struct Page<'a> {
    root: Node<'a>,
}

impl<'a> Page<'a> {
    /// Checks a page.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML or is an HTML page. Old
    /// receivers answer with an HTML error page for pages that they do not have.
    pub fn parse(body: &'a [u8]) -> Result<Self, Error> {
        check_well_formed(body)?;
        let root = Node::new(body)
            .children()
            .next()
            .ok_or(Error::HtmlResponse)?;
        if root.is("html") {
            return Err(Error::HtmlResponse);
        }
        Ok(Self { root: root.node() })
    }

    /// Follows `path` and calls `on_value` for the first element found.
    /// Returns false if there is no such element.
    fn find(&self, path: &[Step], zone: Zone, on_value: &mut impl FnMut(Option<&str>)) -> bool {
        find(self.root, path, zone, Leaf::Text, on_value)
    }
}

/// Friendly name on the page at [`zone_path`] of the main zone, trimmed.
/// `None` if the page has no name or the name is empty.
#[must_use]
pub fn friendly_name(page: &Page<'_>) -> Option<String> {
    let mut name = None;
    page.find(&value_of("FriendlyName"), Zone::Main, &mut |value| {
        name = value
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_owned);
    });
    name
}

/// Path of the status page of a zone.
#[must_use]
pub const fn status_path(zone: Zone) -> &'static str {
    match zone {
        Zone::Main => "/goform/formMainZone_MainZoneXmlStatus.xml",
        Zone::Zone2 => "/goform/formZone2_Zone2XmlStatus.xml",
        Zone::Zone3 => "/goform/formZone3_Zone3XmlStatus.xml",
    }
}

/// Path of the second page of a zone. It has the power, the sound mode and
/// the eco mode.
#[must_use]
pub const fn zone_path(zone: Zone) -> &'static str {
    match zone {
        Zone::Main => zone_page!(),
        Zone::Zone2 => concat!(zone_page!(), "?ZoneName=Zone2"),
        Zone::Zone3 => concat!(zone_page!(), "?ZoneName=Zone3"),
    }
}

/// Steps to the `value` element of a tag.
const fn value_of(tag: &'static str) -> [Step; 2] {
    [Step::Tag(tag), Step::Tag("value")]
}

const POWER: [&[Step]; 2] = [&value_of("ZonePower"), &value_of("Power")];
const ECO_MODE: [&[Step]; 1] = [&value_of("ECOMode")];
const INPUT: [&[Step]; 1] = [&value_of("InputFuncSelect")];
const VOLUME: [&[Step]; 1] = [&value_of("MasterVolume")];
const MUTE: [&[Step]; 1] = [&value_of("Mute")];
const SELECT_SURROUND: [&[Step]; 1] = [&value_of("selectSurround")];
const SURR_MODE: [&[Step]; 1] = [&value_of("SurrMode")];

/// Calls `on_value` for each value found on the pages of a zone.
///
/// `status` is the page at [`status_path`] and `zone_page` the page at
/// [`zone_path`]. Either is `None` if it could not be read.
///
/// - Power and eco mode are taken from `status` first, then from `zone_page`.
///   An empty value is skipped.
/// - Source, volume and mute are taken from `status`. For the main zone
///   `zone_page` is the next place to look. An empty value gives `None`.
/// - The sound mode is read for the main zone only. `selectSurround` is
///   preferred over `SurrMode`.
pub fn extract(
    status: Option<&Page<'_>>,
    zone_page: Option<&Page<'_>>,
    zone: Zone,
    on_value: &mut impl FnMut(Field, Option<&str>),
) {
    let both = [status, zone_page];
    let first_only = [status, None];
    let for_zone = if zone == Zone::Main {
        &both
    } else {
        &first_only
    };

    first(&both, &POWER, zone, true, Field::Power, on_value);
    first(&both, &ECO_MODE, zone, true, Field::EcoMode, on_value);
    first(for_zone, &INPUT, zone, false, Field::InputFunc, on_value);
    first(for_zone, &VOLUME, zone, false, Field::Volume, on_value);
    first(for_zone, &MUTE, zone, false, Field::Muted, on_value);
    if zone == Zone::Main
        && !first(
            &both,
            &SELECT_SURROUND,
            zone,
            false,
            Field::SoundModeRaw,
            on_value,
        )
    {
        first(
            &both,
            &SURR_MODE,
            zone,
            false,
            Field::SoundModeRaw,
            on_value,
        );
    }
}

/// Names in the source lists that are not inputs. `SOURCE` makes a zone play
/// the source of the main zone.
const NOT_SOURCES: [&str; 1] = ["SOURCE"];

/// Calls `on_source` for each source in the lists of the main zone: its name
/// in the XML, the name that the user gave, and if it is in use.
///
/// `InputFuncList` has the names, `RenameSource` the names that the user gave
/// and `SourceDelete` `USE` or `DEL`, in the same order. Some receivers have
/// the lists on the status page, others on the zone page, and some status
/// pages have no `SourceDelete`. The page with all three lists is used, else
/// the first page with names. The name that the user gave is `None` if it is
/// empty. In use is `None` if the page does not say.
pub fn sources(
    status: Option<&Page<'_>>,
    zone_page: Option<&Page<'_>>,
    mut on_source: impl FnMut(&str, Option<&str>, Option<bool>),
) {
    let complete = |[names, _, uses]: &[Option<Node<'_>>; 3]| names.is_some() && uses.is_some();
    // The zone page is read only if the status page lacks a list.
    let first = status.map(source_lists);
    let lists = match first {
        Some(lists) if complete(&lists) => Some(lists),
        _ => match zone_page.map(source_lists) {
            Some(second) if complete(&second) => Some(second),
            second => first.filter(|[names, ..]| names.is_some()).or(second),
        },
    };
    let Some([Some(names), renames, uses]) = lists else {
        return;
    };
    let mut renames = renames.map(Node::children);
    let mut uses = uses.map(Node::children);
    for entry in names.children() {
        let rename = renames.as_mut().and_then(Iterator::next);
        let in_use = uses.as_mut().and_then(Iterator::next);
        let Some(name) = entry.node().text() else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || NOT_SOURCES.contains(&name) {
            continue;
        }
        // The new name is the text of the entry, or of a `value` inside it.
        let rename = rename.and_then(|rename| {
            let node = rename.node();
            node.text()
                .filter(|text| !text.trim().is_empty())
                .or_else(|| node.child("value").and_then(|inner| inner.node().text()))
        });
        let in_use = in_use
            .and_then(|value| value.node().text())
            .and_then(|value| match value.trim() {
                "USE" => Some(true),
                "DEL" => Some(false),
                _ => None,
            });
        on_source(
            name,
            rename
                .as_deref()
                .map(str::trim)
                .filter(|text| !text.is_empty()),
            in_use,
        );
    }
}

/// `InputFuncList`, `RenameSource` and `SourceDelete` of a page.
fn source_lists<'a>(page: &Page<'a>) -> [Option<Node<'a>>; 3] {
    let list = |tag: &str| page.root.child(tag).map(|child| child.node());
    [
        list("InputFuncList"),
        list("RenameSource"),
        list("SourceDelete"),
    ]
}

/// Passes the first element found on the pages as `field`.
///
/// An element without text is a value (`None`), unless `skip_blank` is set.
/// Then it is skipped and the search goes on. Returns true if a value was passed.
fn first(
    pages: &[Option<&Page<'_>>],
    paths: &[&[Step]],
    zone: Zone,
    skip_blank: bool,
    field: Field,
    on_value: &mut impl FnMut(Field, Option<&str>),
) -> bool {
    for page in pages.iter().flatten() {
        for path in paths {
            let mut passed = false;
            page.find(path, zone, &mut |value| {
                if value.is_some() || !skip_blank {
                    on_value(field, value);
                    passed = true;
                }
            });
            if passed {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const STATUS: &[u8] = b"<item><Power><value>ON</value></Power>\
        <InputFuncSelect><value>SAT/CBL</value></InputFuncSelect>\
        <SurrMode><value>PLII Cinema      </value></SurrMode>\
        <MasterVolume><value>-70.0</value></MasterVolume><Mute><value>off</value></Mute></item>";
    const ZONE_PAGE: &[u8] =
        b"<item><Power><value>OFF</value></Power><ZonePower><value>ON</value></ZonePower>\
        <InputFuncSelect><value>CBL/SAT</value></InputFuncSelect>\
        <selectSurround><value>DTS Surround</value></selectSurround>\
        <MasterVolume><value>-60.0</value></MasterVolume><Mute><value>on</value></Mute></item>";

    fn values(
        status: Option<&[u8]>,
        zone_page: Option<&[u8]>,
        zone: Zone,
    ) -> Vec<(Field, Option<String>)> {
        let status = status.map(|b| Page::parse(b).unwrap());
        let zone_page = zone_page.map(|b| Page::parse(b).unwrap());
        let mut out = Vec::new();
        extract(
            status.as_ref(),
            zone_page.as_ref(),
            zone,
            &mut |field, value| {
                out.push((field, value.map(str::to_owned)));
            },
        );
        out
    }

    fn some(field: Field, text: &str) -> (Field, Option<String>) {
        (field, Some(text.to_owned()))
    }

    #[test]
    fn main_zone_reads_both_pages() {
        let found = values(Some(STATUS), Some(ZONE_PAGE), Zone::Main);
        assert_eq!(
            found,
            [
                some(Field::Power, "ON"),
                some(Field::InputFunc, "SAT/CBL"),
                some(Field::Volume, "-70.0"),
                some(Field::Muted, "off"),
                some(Field::SoundModeRaw, "DTS Surround"),
            ]
        );
    }

    #[test]
    fn other_zones_read_the_status_page_for_source_volume_and_mute() {
        let found = values(None, Some(ZONE_PAGE), Zone::Zone2);
        assert_eq!(found, [some(Field::Power, "ON")]);
        let found = values(Some(STATUS), Some(ZONE_PAGE), Zone::Zone3);
        assert_eq!(
            found,
            [
                some(Field::Power, "ON"),
                some(Field::InputFunc, "SAT/CBL"),
                some(Field::Volume, "-70.0"),
                some(Field::Muted, "off"),
            ]
        );
    }

    #[test]
    fn zone_power_is_preferred_on_the_same_page() {
        let page = b"<item><Power><value>OFF</value></Power><ZonePower><value>ON</value></ZonePower></item>";
        assert_eq!(
            values(Some(page), None, Zone::Main),
            [some(Field::Power, "ON")]
        );
    }

    #[test]
    fn empty_power_is_skipped_but_empty_volume_is_a_value() {
        let status =
            b"<item><Power><value></value></Power><MasterVolume><value/></MasterVolume></item>";
        let found = values(Some(status), Some(ZONE_PAGE), Zone::Main);
        assert!(found.contains(&some(Field::Power, "ON")), "{found:?}");
        assert!(found.contains(&(Field::Volume, None)), "{found:?}");
    }

    #[test]
    fn sound_mode_falls_back_to_surr_mode() {
        let found = values(Some(STATUS), None, Zone::Main);
        assert!(
            found.contains(&some(Field::SoundModeRaw, "PLII Cinema      ")),
            "{found:?}"
        );
        assert!(
            values(Some(STATUS), Some(ZONE_PAGE), Zone::Zone2)
                .iter()
                .all(|(field, _)| *field != Field::SoundModeRaw)
        );
    }

    #[test]
    fn nothing_found_gives_nothing() {
        assert_eq!(values(None, None, Zone::Main).len(), 0);
        assert_eq!(
            values(Some(b"<item/>"), Some(b"<item><x/></item>"), Zone::Main).len(),
            0
        );
    }

    #[test]
    fn rejects_html_and_bad_xml() {
        assert_eq!(
            Page::parse(b"<html><body>no</body></html>").unwrap_err(),
            Error::HtmlResponse
        );
        assert!(matches!(
            Page::parse(b"<item>"),
            Err(Error::InvalidXml { .. })
        ));
    }

    #[test]
    fn friendly_name_is_trimmed() {
        let page = Page::parse(
            b"<item><FriendlyName><value> DENON AVR-3313 </value></FriendlyName></item>",
        )
        .unwrap();
        assert_eq!(friendly_name(&page).as_deref(), Some("DENON AVR-3313"));
        for body in [
            &b"<item/>"[..],
            b"<item><FriendlyName><value> </value></FriendlyName></item>",
        ] {
            assert_eq!(friendly_name(&Page::parse(body).unwrap()), None);
        }
    }

    #[test]
    fn paths() {
        assert_eq!(
            status_path(Zone::Main),
            "/goform/formMainZone_MainZoneXmlStatus.xml"
        );
        assert_eq!(
            status_path(Zone::Zone3),
            "/goform/formZone3_Zone3XmlStatus.xml"
        );
        assert_eq!(
            zone_path(Zone::Main),
            "/goform/formMainZone_MainZoneXml.xml"
        );
        assert_eq!(
            zone_path(Zone::Zone2),
            "/goform/formMainZone_MainZoneXml.xml?ZoneName=Zone2"
        );
    }
}
