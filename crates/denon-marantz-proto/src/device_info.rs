//! `Deviceinfo.xml`: static information that identifies a receiver.

use std::borrow::Cow;

use crate::error::Error;
use crate::xml::{Node, check_well_formed};

/// URL path of `Deviceinfo.xml`.
pub const DEVICE_INFO_PATH: &str = "/goform/Deviceinfo.xml";

/// Values of `Deviceinfo.xml` that the library uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo<'a> {
    /// Version of the communication API, for example `0301`.
    pub comm_api_version: Option<Cow<'a, str>>,
    /// Model name, for example `AVC-X3700H`.
    pub model_name: Option<Cow<'a, str>>,
    /// Number of zones. `1` means the receiver has only the main zone.
    pub device_zones: Option<u8>,
    /// Returns true if the receiver supports Dirac Live.
    pub dirac_live: bool,
}

/// Versions of the communication API that AVR-X receivers report.
const AVR_X_COMM_API_VERSIONS: [&str; 5] = ["0210", "0220", "0250", "0300", "0301"];

/// Model names with one of these parts belong to AVR-X receivers.
const AVR_X_MODEL_PARTS: [&str; 6] = ["AVC-X", "AVC-S", "AVC-A", "AVR-X", "AVR-S", "AVR-A"];

/// Older model names that belong to AVR-X receivers.
const AVR_X_OLD_MODELS: [&str; 22] = [
    "SR5006", "SR5007", "SR5008", "SR5009", "SR6007", "SR6008", "SR6009", "SR6010", "SR6011",
    "SR6012", "SR6013", "SR7007", "SR7008", "SR7009", "SR7010", "SR7011", "SR7012", "SR7013",
    "SR5013", "SR5014", "NR1604", "NR1710",
];

impl<'a> DeviceInfo<'a> {
    /// Reads the answer of `/goform/Deviceinfo.xml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML or is an HTML page.
    pub fn parse(body: &'a [u8]) -> Result<Self, Error> {
        check_well_formed(body)?;
        let root = Node::new(body)
            .children()
            .next()
            .ok_or(Error::HtmlResponse)?;
        if root.is("html") {
            return Err(Error::HtmlResponse);
        }
        let root = root.node();
        let text = |name: &str| root.child(name).and_then(|child| child.node().text());
        Ok(Self {
            comm_api_version: text("CommApiVers"),
            model_name: text("ModelName"),
            device_zones: text("DeviceZones").and_then(|zones| zones.trim().parse().ok()),
            dirac_live: root.contains_descendant("DiracLive"),
        })
    }

    /// Copies the borrowed text, so the value can outlive the response body.
    #[must_use]
    pub fn into_owned(self) -> DeviceInfo<'static> {
        DeviceInfo {
            comm_api_version: self.comm_api_version.map(|v| Cow::Owned(v.into_owned())),
            model_name: self.model_name.map(|v| Cow::Owned(v.into_owned())),
            device_zones: self.device_zones,
            dirac_live: self.dirac_live,
        }
    }

    /// Returns true if the receiver is an AVR-X receiver.
    ///
    /// AVR-X receivers support `AppCommand.xml`. The API version is checked
    /// first, then the model name.
    #[must_use]
    pub fn is_avr_x(&self) -> bool {
        if let Some(version) = &self.comm_api_version
            && AVR_X_COMM_API_VERSIONS.iter().any(|v| version.contains(v))
        {
            return true;
        }
        self.model_name.as_deref().is_some_and(is_avr_x_model)
    }
}

/// Returns true if the model name belongs to an AVR-X receiver.
#[must_use]
pub fn is_avr_x_model(name: &str) -> bool {
    AVR_X_MODEL_PARTS.iter().any(|part| name.contains(part))
        || AVR_X_OLD_MODELS.iter().any(|model| name.contains(model))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_values() {
        let xml = b"<?xml version=\"1.0\"?><Device_Info><CommApiVers>0301</CommApiVers>\
                    <ModelName>AVC-X3700H</ModelName><DeviceZones>2</DeviceZones>\
                    <Caps><x><DiracLive/></x></Caps></Device_Info>";
        let info = DeviceInfo::parse(xml).unwrap();
        assert_eq!(info.comm_api_version.as_deref(), Some("0301"));
        assert_eq!(info.model_name.as_deref(), Some("AVC-X3700H"));
        assert_eq!(info.device_zones, Some(2));
        assert!(info.dirac_live);
        assert!(info.is_avr_x());
    }

    #[test]
    fn into_owned_keeps_the_values() {
        let body = b"<D><ModelName>AVR-X1</ModelName><DeviceZones>2</DeviceZones></D>".to_vec();
        let owned = DeviceInfo::parse(&body).unwrap().into_owned();
        drop(body);
        assert_eq!(owned.model_name.as_deref(), Some("AVR-X1"));
        assert_eq!(owned.device_zones, Some(2));
    }

    #[test]
    fn missing_values_are_none() {
        let info = DeviceInfo::parse(b"<Device_Info></Device_Info>").unwrap();
        assert_eq!(info.comm_api_version, None);
        assert_eq!(info.model_name, None);
        assert_eq!(info.device_zones, None);
        assert!(!info.dirac_live);
        assert!(!info.is_avr_x());
    }

    #[test]
    fn only_direct_children_are_read() {
        let xml = b"<Device_Info><a><ModelName>AVR-X1</ModelName></a></Device_Info>";
        assert_eq!(DeviceInfo::parse(xml).unwrap().model_name, None);
    }

    #[test]
    fn bad_zone_count_is_none() {
        for zones in ["", "x", "-1", "300"] {
            let xml = format!("<D><DeviceZones>{zones}</DeviceZones></D>");
            assert_eq!(
                DeviceInfo::parse(xml.as_bytes()).unwrap().device_zones,
                None,
                "{zones:?}"
            );
        }
        let xml = b"<D><DeviceZones> 3 </DeviceZones></D>";
        assert_eq!(DeviceInfo::parse(xml).unwrap().device_zones, Some(3));
    }

    #[test]
    fn avr_x_by_api_version_or_model() {
        let by = |version: Option<&'static str>, model: Option<&'static str>| DeviceInfo {
            comm_api_version: version.map(Cow::Borrowed),
            model_name: model.map(Cow::Borrowed),
            device_zones: None,
            dirac_live: false,
        };
        assert!(by(Some("0210"), None).is_avr_x());
        assert!(by(Some("x0300y"), Some("Unknown")).is_avr_x());
        assert!(!by(Some("0100"), Some("AVR-1713")).is_avr_x());
        assert!(by(Some("0100"), Some("AVR-X2000")).is_avr_x());
        assert!(by(None, Some("AVC-A10H")).is_avr_x());
        assert!(by(None, Some("DENON AVR-S760H")).is_avr_x());
        assert!(!by(None, Some("AVR-3313")).is_avr_x());
        assert!(!by(None, Some("AVR-3808")).is_avr_x());
        assert!(by(None, Some("SR5006")).is_avr_x());
        assert!(by(None, Some("SR6010")).is_avr_x());
        assert!(by(None, Some("AVR-SR7013")).is_avr_x());
        assert!(by(None, Some("NR1604")).is_avr_x());
        assert!(by(None, Some("NR1710")).is_avr_x());
        assert!(!by(None, Some("NR1711")).is_avr_x());
        assert!(!by(None, Some("SR5010")).is_avr_x());
        assert!(!by(None, Some("avr-x2000")).is_avr_x());
        assert!(!by(None, None).is_avr_x());
    }

    #[test]
    fn entities_in_text() {
        let xml = b"<D><ModelName>A&amp;B &#65;</ModelName></D>";
        assert_eq!(
            DeviceInfo::parse(xml).unwrap().model_name.as_deref(),
            Some("A&B A")
        );
    }

    #[test]
    fn rejects_html_and_bad_xml() {
        assert_eq!(
            DeviceInfo::parse(b"<html><body>x</body></html>"),
            Err(Error::HtmlResponse)
        );
        assert!(matches!(
            DeviceInfo::parse(b"<a><b></a>"),
            Err(Error::InvalidXml { .. })
        ));
        assert!(matches!(
            DeviceInfo::parse(b""),
            Err(Error::InvalidXml { .. })
        ));
        assert!(matches!(
            DeviceInfo::parse(b"just text"),
            Err(Error::InvalidXml { .. })
        ));
        let dtd = b"<!DOCTYPE a [<!ENTITY x \"y\">]><a>&x;</a>";
        assert!(matches!(
            DeviceInfo::parse(dtd),
            Err(Error::InvalidXml { what: "DTD", .. })
        ));
    }
}
