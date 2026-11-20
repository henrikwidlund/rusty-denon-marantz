//! `Deviceinfo.xml`: static information that identifies a receiver.

use std::borrow::Cow;

use crate::brand::Brand;
use crate::capabilities::{self, DeviceFeatures, DeviceRanges};
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
    /// Brand from `BrandCode`. Older receivers have none.
    pub brand: Option<Brand>,
    /// What the file says about a tuner.
    pub tuner: TunerInfo,
    /// Number of quick selects (smart selects on Marantz) that the file
    /// lists. `0` if it says that there are none, `None` if it does not say.
    /// A receiver can accept more than it lists.
    pub quick_select_count: Option<u8>,
    /// The queries that the file lists. `None` if it has no list, as on
    /// receivers older than about 2015.
    pub queries: Option<ListedQueries>,
    /// The features that the file lists. `None` for files older than about
    /// 2016, which lack most entries.
    pub features: Option<DeviceFeatures>,
    /// The value ranges that the file lists.
    pub ranges: DeviceRanges,
}

/// The `AppCommand` queries that `Deviceinfo.xml` lists as supported. Each
/// field is true if the file lists the query with the value 1.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
#[expect(clippy::struct_excessive_bools, reason = "each field is one query")]
pub struct ListedQueries {
    /// `GetToneControl`.
    pub tone_control: bool,
    /// `GetAudyssey` in `AppCommand0300.xml`.
    pub audyssey: bool,
    /// `GetVideoInfo` in `AppCommand0300.xml`.
    pub video_info: bool,
    /// `GetAudioInfo` in `AppCommand0300.xml`.
    pub audio_info: bool,
    /// `GetInputSignal` in `AppCommand0300.xml`.
    pub input_signal: bool,
    /// `GetActiveSpeaker` in `AppCommand0300.xml`.
    pub active_speaker: bool,
    /// `GetOutputSettings` in `AppCommand0300.xml`.
    pub output_settings: bool,
}

/// Names of the queries of [`ListedQueries`].
const QUERY_NAMES: [&str; 7] = [
    "GetToneControl",
    "GetAudyssey",
    "GetVideoInfo",
    "GetAudioInfo",
    "GetInputSignal",
    "GetActiveSpeaker",
    "GetOutputSettings",
];

impl ListedQueries {
    /// Returns true if the file lists the query with this name, for example
    /// `GetVideoInfo`. False for a name that this type does not know.
    #[must_use]
    pub fn lists(&self, name: &str) -> bool {
        let mut copy = *self;
        copy.field_mut(name).is_some_and(|listed| *listed)
    }

    fn field_mut(&mut self, name: &str) -> Option<&mut bool> {
        Some(match name {
            "GetToneControl" => &mut self.tone_control,
            "GetAudyssey" => &mut self.audyssey,
            "GetVideoInfo" => &mut self.video_info,
            "GetAudioInfo" => &mut self.audio_info,
            "GetInputSignal" => &mut self.input_signal,
            "GetActiveSpeaker" => &mut self.active_speaker,
            "GetOutputSettings" => &mut self.output_settings,
            _ => return None,
        })
    }

    /// Reads the `Commands` of the blocks under `DeviceCapabilities/Setup`.
    /// The list counts only if it has `StatusInfo`: older files have some of
    /// the blocks, but do not list the queries. Tone control is listed for the
    /// receiver or, on some models, only for a zone, so the whole file is
    /// searched for it.
    fn parse(root: Node<'_>) -> Option<Self> {
        let setup = root.child("DeviceCapabilities")?.node().child("Setup")?;
        let mut queries = Self::default();
        let mut has_status_info = false;
        for block in setup.node().children() {
            has_status_info |= block.is("StatusInfo");
            if !["StatusInfo", "Audyssey", "OutputSettings"]
                .iter()
                .any(|name| block.is(name))
            {
                continue;
            }
            let Some(list) = block.node().child("Commands") else {
                continue;
            };
            for query in list.node().children() {
                let Some(name) = QUERY_NAMES.iter().find(|name| query.is(name)) else {
                    continue;
                };
                if let Some(listed) = queries.field_mut(name) {
                    *listed = query.node().text().is_some_and(|text| text.trim() == "1");
                }
            }
        }
        if !has_status_info {
            return None;
        }
        queries.tone_control = root
            .descendants("GetToneControl")
            .any(|query| query.node().text().is_some_and(|text| text.trim() == "1"));
        Some(queries)
    }
}

/// What `Deviceinfo.xml` says about a tuner.
///
/// It tells if the receiver has a tuner, when the file says so. It does not
/// tell if the status page of the tuner has the band and the frequency.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum TunerInfo {
    /// The file does not say. Receivers older than about 2013 do not. Newer
    /// receivers list a tuner source without a tuner block, also when only
    /// the models of some regions have a tuner.
    #[default]
    Unknown,
    /// The receiver has no tuner.
    Absent,
    /// The receiver has a tuner. If the file lists no bands, all bands are
    /// false.
    Present(TunerBands),
}

/// The bands of a tuner that `Deviceinfo.xml` lists.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
#[expect(
    clippy::struct_excessive_bools,
    reason = "each field is one band of a set"
)]
pub struct TunerBands {
    /// AM.
    pub am: bool,
    /// FM.
    pub fm: bool,
    /// DAB.
    pub dab: bool,
    /// A listed band that [`TunerBand`] does not have.
    pub other: bool,
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
        !(self.am || self.fm || self.dab || self.other)
    }
}

impl FromIterator<TunerBand> for TunerBands {
    fn from_iter<I: IntoIterator<Item = TunerBand>>(bands: I) -> Self {
        bands.into_iter().fold(Self::default(), Self::with)
    }
}

impl TunerInfo {
    /// Reads the tuner from the root of `Deviceinfo.xml`. Older receivers
    /// describe it in `TunerOperation`, where `Control` 0 means none. A
    /// missing or empty `Control` is read as if there were no
    /// `TunerOperation`. Newer receivers only link the tuner source to
    /// `TunerOperation`, also receivers without a tuner, so that link is not
    /// read.
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
                        .filter_map(|band| band.node().child("Name")?.node().text())
                        .fold(TunerBands::default(), |bands, name| match name.trim() {
                            "" => bands,
                            name => TunerBand::from_code(name).map_or(
                                TunerBands {
                                    other: true,
                                    ..bands
                                },
                                |band| bands.with(band),
                            ),
                        });
                    return Self::Present(bands);
                }
                Some(_) => return Self::Absent,
            }
        }
        Self::Unknown
    }
}

/// Reads `MaxQuickSelect` from the first `QuickSelect` element. Each zone has
/// one, and in all captures they are the same. `Control` 0 means none.
fn quick_select_count(root: Node<'_>) -> Option<u8> {
    let block = root.descendants("QuickSelect").next()?.node();
    let text = |name: &str| block.child(name).and_then(|child| child.node().text());
    if text("Control").as_deref().map(str::trim) == Some("0") {
        return Some(0);
    }
    text("MaxQuickSelect").and_then(|count| count.trim().parse().ok())
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
        let (features, ranges) = capabilities::parse(root);
        Ok(Self {
            comm_api_version: text("CommApiVers"),
            model_name: text("ModelName"),
            device_zones: text("DeviceZones").and_then(|zones| zones.trim().parse().ok()),
            dirac_live: root.contains_descendant("DiracLive"),
            category: text("CategoryName"),
            mac_address: text("MacAddress"),
            brand: text("BrandCode").and_then(|code| Brand::from_code(&code)),
            tuner: TunerInfo::parse(root),
            quick_select_count: quick_select_count(root),
            queries: ListedQueries::parse(root),
            features,
            ranges,
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
            brand: self.brand,
            tuner: self.tuner,
            quick_select_count: self.quick_select_count,
            queries: self.queries,
            features: self.features,
            ranges: self.ranges,
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

/// Returns true if the body is the error page that old receivers send for
/// `Deviceinfo.xml`, for example "Form Deviceinfo.xml is not defined". Such
/// a receiver is not an AVR-X receiver.
#[must_use]
pub fn is_old_model_page(body: &[u8]) -> bool {
    const NEEDLE: &[u8] = b"is not defined";
    body.trim_ascii_start()
        .get(..5)
        .is_some_and(|start| start.eq_ignore_ascii_case(b"<html"))
        && body.windows(NEEDLE.len()).any(|part| part == NEEDLE)
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
                    <MacAddress>0123456789AB</MacAddress><BrandCode>1</BrandCode>\
                    <Caps><x><DiracLive/></x></Caps></Device_Info>";
        let info = DeviceInfo::parse(xml).unwrap();
        assert_eq!(info.mac_address.as_deref(), Some("0123456789AB"));
        assert_eq!(info.comm_api_version.as_deref(), Some("0301"));
        assert_eq!(info.model_name.as_deref(), Some("AVC-X3700H"));
        assert_eq!(info.device_zones, Some(2));
        assert_eq!(info.brand, Some(Brand::Marantz));
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
        assert_eq!(info.brand, None);
        assert!(!info.dirac_live);
        assert_eq!(info.tuner, TunerInfo::Unknown);
        assert_eq!(info.quick_select_count, None);
        assert_eq!(info.queries, None);
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
             <Band><Name> </Name></Band><Band/></BandList>",
        );
        assert_eq!(
            present,
            TunerInfo::Present(TunerBands::default().with(TunerBand::Fm))
        );
        let other =
            tuner("<Control>1</Control><BandList><Band><Name>DAB+</Name></Band></BandList>");
        let TunerInfo::Present(other) = other else {
            panic!("{other:?}");
        };
        assert!(other.other && !other.is_empty() && !other.contains(TunerBand::Dab));
        assert_eq!(tuner("<Control>0</Control>"), TunerInfo::Absent);
        assert_eq!(tuner("<Control/>"), TunerInfo::Unknown);
        assert_eq!(tuner("<Control> </Control>"), TunerInfo::Unknown);
        assert_eq!(tuner("<BandList/>"), TunerInfo::Unknown);
        let linked = b"<Device_Info><TunerOperation/>\
                       <Source><RelatedFunc>TunerOperation</RelatedFunc></Source></Device_Info>";
        let linked = DeviceInfo::parse(linked).unwrap().tuner;
        assert_eq!(linked, TunerInfo::Unknown);
    }

    #[test]
    fn quick_select_count_from_the_block() {
        let count = |block: &str| {
            let xml = format!("<Device_Info><QuickSelect>{block}</QuickSelect></Device_Info>");
            DeviceInfo::parse(xml.as_bytes())
                .unwrap()
                .quick_select_count
        };
        assert_eq!(
            count("<Control>1</Control><MaxQuickSelect> 5 </MaxQuickSelect>"),
            Some(5)
        );
        assert_eq!(count("<Control>0</Control>"), Some(0));
        assert_eq!(count("<Control>1</Control>"), None);
        assert_eq!(count("<MaxQuickSelect>x</MaxQuickSelect>"), None);
        assert_eq!(count("<MaxQuickSelect>300</MaxQuickSelect>"), None);
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
    fn old_model_page() {
        let page = b"<html><head><title>Document Error: Data follows</title></head>\
                     <body><p>Form Deviceinfo.xml is not defined</p></body></html>";
        assert!(is_old_model_page(page));
        assert!(is_old_model_page(b" \n<HTML>is not defined"));
        assert!(!is_old_model_page(b"<html><body>Not found</body></html>"));
        assert!(!is_old_model_page(
            b"<Device_Info>is not defined</Device_Info>"
        ));
        assert!(!is_old_model_page(b""));
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
