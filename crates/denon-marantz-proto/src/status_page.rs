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

/// Main zone page that has the values of all zones, for the main zone.
const MAIN_ZONE_PAGE: &str = "/goform/formMainZone_MainZoneXml.xml";

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
        Zone::Main => MAIN_ZONE_PAGE,
        Zone::Zone2 => "/goform/formMainZone_MainZoneXml.xml?ZoneName=Zone2",
        Zone::Zone3 => "/goform/formMainZone_MainZoneXml.xml?ZoneName=Zone3",
    }
}

const POWER: [&[Step]; 2] = [
    &[Step::Tag("ZonePower"), Step::Tag("value")],
    &[Step::Tag("Power"), Step::Tag("value")],
];
const ECO_MODE: [&[Step]; 1] = [&[Step::Tag("ECOMode"), Step::Tag("value")]];
const INPUT: [&[Step]; 1] = [&[Step::Tag("InputFuncSelect"), Step::Tag("value")]];
const VOLUME: [&[Step]; 1] = [&[Step::Tag("MasterVolume"), Step::Tag("value")]];
const MUTE: [&[Step]; 1] = [&[Step::Tag("Mute"), Step::Tag("value")]];
const SELECT_SURROUND: [&[Step]; 1] = [&[Step::Tag("selectSurround"), Step::Tag("value")]];
const SURR_MODE: [&[Step]; 1] = [&[Step::Tag("SurrMode"), Step::Tag("value")]];

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

    first_non_empty(&both, &POWER, zone, |value| {
        on_value(Field::Power, Some(value));
    });
    first_non_empty(&both, &ECO_MODE, zone, |value| {
        on_value(Field::EcoMode, Some(value));
    });
    first_found(for_zone, &INPUT, zone, |value| {
        on_value(Field::InputFunc, value);
    });
    first_found(for_zone, &VOLUME, zone, |value| {
        on_value(Field::Volume, value);
    });
    first_found(for_zone, &MUTE, zone, |value| {
        on_value(Field::Muted, value);
    });
    if zone == Zone::Main
        && !first_found(&both, &SELECT_SURROUND, zone, |value| {
            on_value(Field::SoundModeRaw, value);
        })
    {
        first_found(&both, &SURR_MODE, zone, |value| {
            on_value(Field::SoundModeRaw, value);
        });
    }
}

/// Passes the first element found on the pages, with or without text.
/// Returns true if one was found.
fn first_found(
    pages: &[Option<&Page<'_>>],
    paths: &[&[Step]],
    zone: Zone,
    mut on_value: impl FnMut(Option<&str>),
) -> bool {
    for page in pages.iter().flatten() {
        for path in paths {
            if page.find(path, zone, &mut on_value) {
                return true;
            }
        }
    }
    false
}

/// Passes the first element that has text. Empty elements are skipped.
fn first_non_empty(
    pages: &[Option<&Page<'_>>],
    paths: &[&[Step]],
    zone: Zone,
    mut on_value: impl FnMut(&str),
) -> bool {
    let mut done = false;
    for page in pages.iter().flatten() {
        for path in paths {
            page.find(path, zone, &mut |value| {
                if let Some(value) = value {
                    on_value(value);
                    done = true;
                }
            });
            if done {
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
