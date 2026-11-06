//! UPnP device description of a receiver (`description.xml`).
//!
//! It names the manufacturer, the model and the serial number. Discovery and
//! setup both use it.

use std::borrow::Cow;

use crate::brand::Brand;
use crate::error::Error;
use crate::xml::{Child, Node, check_well_formed};

/// Device types that carry the model and serial number of a receiver.
const DEVICE_TYPES: [&str; 2] = [
    "urn:schemas-upnp-org:device:MediaRenderer:1",
    "urn:schemas-upnp-org:device:MediaServer:1",
];

/// Manufacturer names of supported receivers. Case is ignored.
const MANUFACTURERS: [&str; 3] = ["Denon", "DENON PROFESSIONAL", "Marantz"];

/// Values of a device description that the library uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceDescription<'a> {
    /// Manufacturer, for example `Denon`.
    pub manufacturer: Cow<'a, str>,
    /// Model, for example `Denon AVR-X1600H`.
    pub model_name: Option<Cow<'a, str>>,
    /// Serial number.
    pub serial_number: Option<Cow<'a, str>>,
    /// Name that the user sees, for example `Denon AVR-X1600H`.
    pub friendly_name: Option<Cow<'a, str>>,
    /// URL of the web interface.
    pub presentation_url: Option<Cow<'a, str>>,
}

impl<'a> DeviceDescription<'a> {
    /// Reads a device description.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML or is an HTML page, or
    /// [`Error::UnsupportedDevice`] if the device is not from a supported
    /// manufacturer or has no media renderer or media server part.
    pub fn parse(body: &'a [u8]) -> Result<Self, Error> {
        check_well_formed(body)?;
        let root = Node::new(body)
            .children()
            .next()
            .ok_or(Error::HtmlResponse)?;
        if root.is("html") {
            return Err(Error::HtmlResponse);
        }
        let device = root
            .node()
            .child("device")
            .ok_or(Error::UnsupportedDevice)?;
        let manufacturer = text(&device, "manufacturer").ok_or(Error::UnsupportedDevice)?;
        if !MANUFACTURERS
            .iter()
            .any(|name| name.eq_ignore_ascii_case(&manufacturer))
        {
            return Err(Error::UnsupportedDevice);
        }
        let part = find_part(device).ok_or(Error::UnsupportedDevice)?;
        Ok(Self {
            manufacturer,
            model_name: text(&part, "modelName"),
            serial_number: text(&part, "serialNumber"),
            friendly_name: text(&part, "friendlyName"),
            presentation_url: text(&part, "presentationURL"),
        })
    }

    /// Brand of the receiver.
    #[must_use]
    pub fn brand(&self) -> Brand {
        Brand::from_manufacturer(Some(&self.manufacturer))
    }

    /// Copies the borrowed text, so the value can outlive the response body.
    #[must_use]
    pub fn into_owned(self) -> DeviceDescription<'static> {
        let own = |value: Option<Cow<'_, str>>| value.map(|v| Cow::Owned(v.into_owned()));
        DeviceDescription {
            manufacturer: Cow::Owned(self.manufacturer.into_owned()),
            model_name: own(self.model_name),
            serial_number: own(self.serial_number),
            friendly_name: own(self.friendly_name),
            presentation_url: own(self.presentation_url),
        }
    }
}

/// The device that has the model data: the device itself, or the first
/// device in its list that is a supported type and has a serial number.
fn find_part(device: Child<'_>) -> Option<Child<'_>> {
    if is_supported_type(&device) {
        return Some(device);
    }
    device
        .node()
        .child("deviceList")?
        .node()
        .children()
        .find(|part| is_supported_type(part) && part.node().child("serialNumber").is_some())
}

fn is_supported_type(device: &Child<'_>) -> bool {
    text(device, "deviceType").is_some_and(|kind| DEVICE_TYPES.contains(&kind.as_ref()))
}

/// Text of a child element, trimmed.
fn text<'a>(device: &Child<'a>, name: &str) -> Option<Cow<'a, str>> {
    let value = device.node().child(name)?.node().text()?;
    let trimmed = value.trim();
    if trimmed.len() == value.len() {
        Some(value)
    } else {
        Some(Cow::Owned(trimmed.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(device: &str) -> Vec<u8> {
        format!("<root xmlns=\"urn:schemas-upnp-org:device-1-0\"><device>{device}</device></root>")
            .into_bytes()
    }

    #[test]
    fn device_itself_is_a_renderer() {
        let body = wrap(
            "<deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>\
             <manufacturer>Marantz</manufacturer><modelName>Marantz NR1711</modelName>\
             <serialNumber> S1 </serialNumber><friendlyName>Living room</friendlyName>\
             <presentationURL>http://10.0.0.5/</presentationURL>",
        );
        let description = DeviceDescription::parse(&body).unwrap();
        assert_eq!(description.manufacturer, "Marantz");
        assert_eq!(description.model_name.as_deref(), Some("Marantz NR1711"));
        assert_eq!(description.serial_number.as_deref(), Some("S1"));
        assert_eq!(description.friendly_name.as_deref(), Some("Living room"));
        assert_eq!(
            description.presentation_url.as_deref(),
            Some("http://10.0.0.5/")
        );
        assert_eq!(description.brand(), Brand::Marantz);
    }

    #[test]
    fn part_in_the_device_list_needs_a_serial_number() {
        let body = wrap(
            "<deviceType>urn:schemas-denon-com:device:AiosDevice:1</deviceType>\
             <manufacturer>Denon</manufacturer><modelName>Top</modelName>\
             <deviceList>\
             <device><deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>\
             <modelName>No serial</modelName></device>\
             <device><deviceType>urn:schemas-denon-com:device:AiosServices:1</deviceType>\
             <serialNumber>X</serialNumber><modelName>Wrong type</modelName></device>\
             <device><deviceType>urn:schemas-upnp-org:device:MediaServer:1</deviceType>\
             <serialNumber>S2</serialNumber><modelName>Server</modelName></device>\
             </deviceList>",
        );
        let description = DeviceDescription::parse(&body).unwrap();
        assert_eq!(description.model_name.as_deref(), Some("Server"));
        assert_eq!(description.serial_number.as_deref(), Some("S2"));
        assert_eq!(description.brand(), Brand::Denon);
    }

    #[test]
    fn manufacturer_names_ignore_case() {
        for name in [
            "Denon",
            "DENON",
            "denon",
            "DENON PROFESSIONAL",
            "Marantz",
            "MARANTZ",
        ] {
            let body = wrap(&format!(
                "<deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>\
                 <manufacturer>{name}</manufacturer>"
            ));
            assert!(DeviceDescription::parse(&body).is_ok(), "{name}");
        }
    }

    #[test]
    fn other_devices_are_unsupported() {
        let cases = [
            wrap(
                "<deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>\
                 <manufacturer>Sonos</manufacturer>",
            ),
            wrap("<deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>"),
            wrap(
                "<deviceType>urn:schemas-denon-com:device:AiosDevice:1</deviceType>\
                 <manufacturer>Denon</manufacturer>",
            ),
            wrap(
                "<deviceType>urn:schemas-denon-com:device:AiosDevice:1</deviceType>\
                 <manufacturer>Denon</manufacturer><deviceList/>",
            ),
            b"<root></root>".to_vec(),
        ];
        for body in cases {
            assert_eq!(
                DeviceDescription::parse(&body).unwrap_err(),
                Error::UnsupportedDevice,
                "{}",
                String::from_utf8_lossy(&body)
            );
        }
    }

    #[test]
    fn rejects_html_and_bad_xml() {
        assert_eq!(
            DeviceDescription::parse(b"<html><body>no</body></html>").unwrap_err(),
            Error::HtmlResponse
        );
        assert!(matches!(
            DeviceDescription::parse(b"<root>"),
            Err(Error::InvalidXml { .. })
        ));
    }

    #[test]
    fn into_owned_keeps_the_values() {
        let body = wrap(
            "<deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>\
             <manufacturer>Denon</manufacturer><modelName>A &amp; B</modelName>",
        );
        let owned = DeviceDescription::parse(&body).unwrap().into_owned();
        drop(body);
        assert_eq!(owned.model_name.as_deref(), Some("A & B"));
        assert_eq!(owned.manufacturer, "Denon");
    }
}
