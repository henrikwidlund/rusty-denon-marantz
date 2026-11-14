//! `Deviceinfo.xml`: static information that identifies a receiver.

use std::borrow::Cow;

use crate::error::Error;
use crate::values::TunerBand;
use crate::xml::{Node, check_well_formed};

/// URL path of `Deviceinfo.xml`.
pub const DEVICE_INFO_PATH: &str = "/goform/Deviceinfo.xml";

/// Values of `Deviceinfo.xml` that the library uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceInfo<'a> {
    /// Version of the communication API, for example `0301`.
    pub comm_api_version: Option<Cow<'a, str>>,
    /// Model name, for example `AVC-X3700H`.
    pub model_name: Option<Cow<'a, str>>,
    /// Number of zones. `1` means the receiver has only the main zone.
    pub device_zones: Option<u8>,
    /// Returns true if the receiver supports Dirac Live.
    pub dirac_live: bool,
    /// Kind of device, for example `AV RECEIVER`. Older receivers have none.
    pub category: Option<Cow<'a, str>>,
    /// MAC address of the wired network, as 12 hexadecimal digits without
    /// separators.
    pub mac_address: Option<Cow<'a, str>>,
    /// What the file says about a tuner.
    pub tuner: TunerInfo,
}

/// What `Deviceinfo.xml` says about a tuner.
///
/// It tells if the receiver has a tuner. It does not tell if the status page
/// of the tuner has the band and the frequency.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TunerInfo {
    /// The file does not say. Receivers older than about 2013 do not, and
    /// newer receivers without a tuner may not.
    #[default]
    Unknown,
    /// The receiver has no tuner.
    Absent,
    /// The receiver has a tuner. Receivers of about 2017 and later list no
    /// bands, so all bands are false for them.
    Present(TunerBands),
}

/// The bands of a tuner that `Deviceinfo.xml` lists.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct TunerBands {
    /// AM.
    pub am: bool,
    /// FM.
    pub fm: bool,
    /// DAB.
    pub dab: bool,
}

impl TunerBands {
    /// Returns these bands with `band` added.
    #[must_use]
    pub const fn with(mut self, band: TunerBand) -> Self {
        match band {
            TunerBand::Am => self.am = true,
            TunerBand::Fm => self.fm = true,
            TunerBand::Dab => self.dab = true,
        }
        self
    }

    /// Returns true if the band is listed.
    #[must_use]
    pub const fn contains(self, band: TunerBand) -> bool {
        match band {
            TunerBand::Am => self.am,
            TunerBand::Fm => self.fm,
            TunerBand::Dab => self.dab,
        }
    }

    /// Returns true if no band is listed.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        !(self.am || self.fm || self.dab)
    }
}

impl FromIterator<TunerBand> for TunerBands {
    fn from_iter<I: IntoIterator<Item = TunerBand>>(bands: I) -> Self {
        bands.into_iter().fold(Self::default(), Self::with)
    }
}

impl TunerInfo {
    /// Reads the tuner from the root of `Deviceinfo.xml`. Older receivers
    /// describe it in `TunerOperation`, where `Control` 0 means none. Newer
    /// ones only link the tuner source to `TunerOperation`. A missing or empty
    /// `Control` is read as if there were no `TunerOperation`.
    ///
    /// Each zone has a `TunerOperation`. The first one (main zone) is used.
    /// In all captures the blocks of all zones are the same.
    fn parse(root: Node<'_>) -> Self {
        if let Some(operation) = root.descendants("TunerOperation").next() {
            let operation = operation.node();
            let control = operation
                .child("Control")
                .and_then(|control| control.node().text());
            match control.as_deref().map(str::trim) {
                None | Some("") => {}
                Some("1") => {
                    let bands = operation
                        .child("BandList")
                        .into_iter()
                        .flat_map(|list| list.node().children())
                        .filter_map(|band| {
                            TunerBand::from_code(band.node().child("Name")?.node().text()?.trim())
                        })
                        .collect();
                    return Self::Present(bands);
                }
                Some(_) => return Self::Absent,
            }
        }
        let linked = root.descendants("RelatedFunc").any(|func| {
            func.node()
                .text()
                .is_some_and(|text| text.trim() == "TunerOperation")
        });
        if linked {
            Self::Present(TunerBands::default())
        } else {
            Self::Unknown
        }
    }
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
            category: text("CategoryName"),
            mac_address: text("MacAddress"),
            tuner: TunerInfo::parse(root),
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
            category: self.category.map(|v| Cow::Owned(v.into_owned())),
            mac_address: self.mac_address.map(|v| Cow::Owned(v.into_owned())),
            tuner: self.tuner,
        }
    }

    /// Returns false only if the device says that it is another kind of
    /// device than an AV receiver, for example a speaker. A device with no
    /// category counts as an AV receiver, because older receivers have none.
    #[must_use]
    pub fn is_av_receiver(&self) -> bool {
        self.category
            .as_deref()
            .is_none_or(|category| category.trim().eq_ignore_ascii_case("AV RECEIVER"))
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
                    <MacAddress>0123456789AB</MacAddress>\
                    <Caps><x><DiracLive/></x></Caps></Device_Info>";
        let info = DeviceInfo::parse(xml).unwrap();
        assert_eq!(info.mac_address.as_deref(), Some("0123456789AB"));
        assert_eq!(info.comm_api_version.as_deref(), Some("0301"));
        assert_eq!(info.model_name.as_deref(), Some("AVC-X3700H"));
        assert_eq!(info.device_zones, Some(2));
        assert!(info.dirac_live);
        assert!(info.is_avr_x());
    }

    #[test]
    fn into_owned_keeps_the_values() {
        let body = b"<D><ModelName>AVR-X1</ModelName><DeviceZones>2</DeviceZones>\
                     <MacAddress>0123456789AB</MacAddress></D>"
            .to_vec();
        let owned = DeviceInfo::parse(&body).unwrap().into_owned();
        drop(body);
        assert_eq!(owned.model_name.as_deref(), Some("AVR-X1"));
        assert_eq!(owned.device_zones, Some(2));
        assert_eq!(owned.mac_address.as_deref(), Some("0123456789AB"));
    }

    #[test]
    fn missing_values_are_none() {
        let info = DeviceInfo::parse(b"<Device_Info></Device_Info>").unwrap();
        assert_eq!(info.comm_api_version, None);
        assert_eq!(info.model_name, None);
        assert_eq!(info.device_zones, None);
        assert!(!info.dirac_live);
        assert_eq!(info.tuner, TunerInfo::Unknown);
        assert!(!info.is_avr_x());
    }

    #[test]
    fn tuner_control() {
        let tuner = |operation: &str| {
            let xml =
                format!("<Device_Info><TunerOperation>{operation}</TunerOperation></Device_Info>");
            DeviceInfo::parse(xml.as_bytes()).unwrap().tuner
        };
        let present = tuner(
            "<Control> 1 </Control><BandList><Band><Name>FM</Name></Band>\
             <Band><Name>HD</Name></Band></BandList>",
        );
        assert_eq!(
            present,
            TunerInfo::Present(TunerBands::default().with(TunerBand::Fm))
        );
        assert_eq!(tuner("<Control>0</Control>"), TunerInfo::Absent);
        assert_eq!(tuner("<Control/>"), TunerInfo::Unknown);
        assert_eq!(tuner("<Control> </Control>"), TunerInfo::Unknown);
        assert_eq!(tuner("<BandList/>"), TunerInfo::Unknown);
        let linked = b"<Device_Info><TunerOperation/>\
                       <Source><RelatedFunc>TunerOperation</RelatedFunc></Source></Device_Info>";
        let linked = DeviceInfo::parse(linked).unwrap().tuner;
        assert_eq!(linked, TunerInfo::Present(TunerBands::default()));
    }

    #[test]
    fn the_category_tells_if_it_is_an_av_receiver() {
        let info = |category: &str| {
            let xml = format!("<Device_Info><CategoryName>{category}</CategoryName></Device_Info>");
            DeviceInfo::parse(xml.as_bytes()).unwrap().into_owned()
        };
        assert_eq!(info("AV RECEIVER").category.as_deref(), Some("AV RECEIVER"));
        assert!(info("AV RECEIVER").is_av_receiver());
        assert!(info(" av receiver ").is_av_receiver());
        assert!(!info("SOUND BAR").is_av_receiver());
        assert!(!info("").is_av_receiver() || info("").category.is_none());
        // Older receivers have no category.
        let none = DeviceInfo::parse(b"<Device_Info></Device_Info>").unwrap();
        assert_eq!(none.category, None);
        assert!(none.is_av_receiver());
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
            ..DeviceInfo::default()
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
