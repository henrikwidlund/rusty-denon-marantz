//! The video menu of the web interface: the on-screen display, the TV
//! format, the screen saver, the HDMI upscaler, HDCP and the 4K/8K signal
//! format of each input.
//!
//! The pages are `/ajax/video/get_config?type=N` on port 11080 and do not
//! send changes. The meaning of the numbers is the one that the web
//! interface shows.

use std::collections::HashSet;

use crate::error::Error;
use crate::input_pages::source_for_index;
use crate::settings_availability::Availability;
use crate::speaker_tables::{number, root};
use crate::xml::Node;

/// Most sources of a page that are kept.
const MAX_SOURCES: usize = 32;

code_enum! {
    /// Where the web interface shows the volume bar on the screen.
    pub enum OsdVolume {
        Bottom = "1", "Bottom";
        Top = "2", "Top";
        Off = "3", "Off";
    }
}

code_enum! {
    /// When the screen shows what plays on HEOS Music and the tuner.
    pub enum NowPlayingDisplay {
        Always = "1", "Always on";
        Auto = "2", "Off after a while";
    }
}

code_enum! {
    /// The video format of the TV.
    pub enum TvFormat {
        Ntsc = "1", "NTSC";
        Pal = "2", "PAL";
    }
}

code_enum! {
    /// The HDCP version that an input accepts.
    pub enum HdcpMode {
        Auto = "1", "Auto";
        V1_4 = "2", "HDCP 1.4";
        V2_3 = "3", "HDCP 2.3";
        Custom = "4", "Custom";
    }
}

code_enum! {
    /// The signal format of an HDMI input.
    pub enum SignalFormat {
        Standard = "1", "Standard";
        Enhanced = "2", "Enhanced";
        Custom = "3", "Custom";
        Enhanced8k = "4", "8K enhanced";
    }
}

/// The settings of the on-screen display.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OnScreenDisplay {
    /// Where the volume bar is.
    pub volume: Option<OsdVolume>,
    /// The information display is on.
    pub info: Option<bool>,
    /// When what plays is shown.
    pub now_playing: Option<NowPlayingDisplay>,
    /// The 21:9 display mode is on.
    pub wide_cinematic: Option<bool>,
}

/// A setting of one input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceSetting<T> {
    /// Index of the source on the web interface.
    pub index: u8,
    /// Telnet name of the source, if the index is known.
    pub source: Option<&'static str>,
    /// Name of the source.
    pub name: String,
    /// The value. `None` if the number is not known.
    pub value: Option<T>,
    /// The receiver offers the custom choice for this input.
    pub custom_offered: bool,
    /// If the user can change the setting. An input without an HDMI input is
    /// shown but blocked. `None` if the page does not say.
    pub availability: Option<Availability>,
}

/// The pages of the video menu that are read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum VideoMenuPage {
    /// The on-screen display.
    OnScreenDisplay,
    /// The TV format.
    TvFormat,
    /// The HDCP version of each input.
    Hdcp,
    /// The screen saver.
    ScreenSaver,
    /// The HDMI upscaler.
    HdmiUpscaler,
    /// The 4K and 8K signal format of each input.
    SignalFormat,
}

impl VideoMenuPage {
    /// All pages, in the order that they are read.
    pub const ALL: [Self; 6] = [
        Self::OnScreenDisplay,
        Self::TvFormat,
        Self::Hdcp,
        Self::ScreenSaver,
        Self::HdmiUpscaler,
        Self::SignalFormat,
    ];

    /// Path of the page.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::OnScreenDisplay => "/ajax/video/get_config?type=7",
            Self::TvFormat => "/ajax/video/get_config?type=9",
            Self::Hdcp => "/ajax/video/get_config?type=11",
            Self::ScreenSaver => "/ajax/video/get_config?type=12",
            Self::HdmiUpscaler => "/ajax/video/get_config?type=13",
            Self::SignalFormat => "/ajax/video/get_config?type=14",
        }
    }

    /// The name of the root element of the page.
    const fn root_name(self) -> &'static str {
        match self {
            Self::OnScreenDisplay => "OnScreenDisplay",
            Self::TvFormat => "TVFormat",
            Self::Hdcp => "HDCPSetup",
            Self::ScreenSaver => "ScreenSaver",
            Self::HdmiUpscaler => "HDMIScaler",
            Self::SignalFormat => "FourKEightKSignalFormatPerInput",
        }
    }

    /// Reads the page.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML, is an HTML page, is not
    /// the page that was asked for, or has no value that is known. A page
    /// without content must not replace what the state has.
    pub fn parse(self, body: &[u8]) -> Result<VideoMenuPart, Error> {
        let root = root(body)?;
        if !root.is(self.root_name()) {
            return Err(Error::UnexpectedDocument {
                expected: self.root_name(),
            });
        }
        let node = root.node();
        let text = node.text();
        let text = text.as_deref().map(str::trim);
        let part = match self {
            Self::OnScreenDisplay => VideoMenuPart::OnScreenDisplay(OnScreenDisplay {
                volume: child_with(node, "Volume", OsdVolume::from_code),
                info: child_with(node, "Info", flag),
                now_playing: child_with(node, "NowPlaying", NowPlayingDisplay::from_code),
                wide_cinematic: child_with(node, "WideCinematicDisplay", flag),
            }),
            Self::TvFormat => VideoMenuPart::TvFormat(text.and_then(TvFormat::from_code)),
            Self::ScreenSaver => VideoMenuPart::ScreenSaver(text.and_then(flag)),
            // The web interface writes 1 for on and 10 for off.
            Self::HdmiUpscaler => VideoMenuPart::HdmiUpscaler(match text {
                Some("1") => Some(true),
                Some("10") => Some(false),
                _ => None,
            }),
            Self::Hdcp => {
                VideoMenuPart::Hdcp(source_settings(node, "Value", "Limit", HdcpMode::from_code))
            }
            Self::SignalFormat => VideoMenuPart::SignalFormat(source_settings(
                node,
                "Format",
                "MaxResolution",
                SignalFormat::from_code,
            )),
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

/// What a page of the video menu tells.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum VideoMenuPart {
    /// The on-screen display.
    OnScreenDisplay(OnScreenDisplay),
    /// The TV format.
    TvFormat(Option<TvFormat>),
    /// The HDCP version of each input.
    Hdcp(Vec<SourceSetting<HdcpMode>>),
    /// The screen saver is on.
    ScreenSaver(Option<bool>),
    /// The HDMI upscaler is on.
    HdmiUpscaler(Option<bool>),
    /// The signal format of each input.
    SignalFormat(Vec<SourceSetting<SignalFormat>>),
}

impl VideoMenuPart {
    /// Returns true if the page gave at least one known value.
    fn has_content(&self) -> bool {
        match self {
            Self::OnScreenDisplay(osd) => *osd != OnScreenDisplay::default(),
            Self::TvFormat(value) => value.is_some(),
            Self::ScreenSaver(value) | Self::HdmiUpscaler(value) => value.is_some(),
            Self::Hdcp(list) => list.iter().any(|setting| setting.value.is_some()),
            Self::SignalFormat(list) => list.iter().any(|setting| setting.value.is_some()),
        }
    }
}

/// The video menu as the receiver told it. A value is `None` until its page
/// was read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VideoMenu {
    /// The on-screen display.
    pub on_screen_display: Option<OnScreenDisplay>,
    /// The TV format.
    pub tv_format: Option<TvFormat>,
    /// The screen saver is on.
    pub screen_saver: Option<bool>,
    /// The HDMI upscaler is on.
    pub hdmi_upscaler: Option<bool>,
    /// The HDCP version of each input.
    pub hdcp: Vec<SourceSetting<HdcpMode>>,
    /// The 4K and 8K signal format of each input.
    pub signal_format: Vec<SourceSetting<SignalFormat>>,
}

impl VideoMenu {
    /// Stores what a page told. Returns true if something was different.
    pub(crate) fn apply(&mut self, part: VideoMenuPart) -> bool {
        fn put<T: PartialEq>(slot: &mut T, value: T) -> bool {
            if *slot == value {
                false
            } else {
                *slot = value;
                true
            }
        }
        match part {
            VideoMenuPart::OnScreenDisplay(value) => put(&mut self.on_screen_display, Some(value)),
            VideoMenuPart::TvFormat(value) => put(&mut self.tv_format, value),
            VideoMenuPart::ScreenSaver(value) => put(&mut self.screen_saver, value),
            VideoMenuPart::HdmiUpscaler(value) => put(&mut self.hdmi_upscaler, value),
            VideoMenuPart::Hdcp(value) => put(&mut self.hdcp, value),
            VideoMenuPart::SignalFormat(value) => put(&mut self.signal_format, value),
        }
    }
}

/// Reads the text of the child `name` with `read`, without a copy.
fn child_with<T>(node: Node<'_>, name: &str, read: impl FnOnce(&str) -> Option<T>) -> Option<T> {
    let text = node.child(name)?.node().text()?;
    read(text.trim())
}

/// `1` is on and `0` is off.
fn flag(text: &str) -> Option<bool> {
    match text {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// Reads the `Source` elements. `value_name` is the element with the value
/// and `custom_name` the element that is `1` if the custom choice is offered.
/// A source is listed once, and the first entry counts.
fn source_settings<T>(
    node: Node<'_>,
    value_name: &str,
    custom_name: &str,
    from_code: fn(&str) -> Option<T>,
) -> Vec<SourceSetting<T>> {
    let mut settings = Vec::new();
    let mut seen = HashSet::new();
    for source in node.children().filter(|c| c.is("Source")) {
        let Some(index) = source
            .attribute("index")
            .and_then(|index| index.trim().parse::<u8>().ok())
        else {
            continue;
        };
        if !seen.insert(index) {
            continue;
        }
        if settings.len() == MAX_SOURCES {
            break;
        }
        let content = source.node();
        settings.push(SourceSetting {
            index,
            source: source_for_index(index),
            name: child_with(content, "Name", |text| Some(text.to_owned())).unwrap_or_default(),
            value: child_with(content, value_name, from_code),
            custom_offered: number::<u8>(content, custom_name) == Some(1),
            availability: source
                .attribute("display")
                .and_then(|display| display.trim().parse::<u8>().ok())
                .and_then(Availability::from_code),
        });
    }
    settings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(page: VideoMenuPage, body: &[u8]) -> VideoMenuPart {
        page.parse(body).unwrap()
    }

    #[test]
    fn pages_of_the_a10h() {
        let osd = page(
            VideoMenuPage::OnScreenDisplay,
            include_bytes!("../tests/data/xml/AVC-A10H-web-video-osd.xml"),
        );
        assert_eq!(
            osd,
            VideoMenuPart::OnScreenDisplay(OnScreenDisplay {
                volume: Some(OsdVolume::Off),
                info: Some(false),
                now_playing: Some(NowPlayingDisplay::Auto),
                wide_cinematic: Some(false),
            })
        );
        assert_eq!(
            page(
                VideoMenuPage::TvFormat,
                include_bytes!("../tests/data/xml/AVC-A10H-web-video-tv-format.xml")
            ),
            VideoMenuPart::TvFormat(Some(TvFormat::Ntsc))
        );
        assert_eq!(
            page(
                VideoMenuPage::ScreenSaver,
                include_bytes!("../tests/data/xml/AVC-A10H-web-video-screen-saver.xml")
            ),
            VideoMenuPart::ScreenSaver(Some(false))
        );
        assert_eq!(
            page(
                VideoMenuPage::HdmiUpscaler,
                include_bytes!("../tests/data/xml/AVC-A10H-web-video-hdmi-scaler.xml")
            ),
            VideoMenuPart::HdmiUpscaler(Some(false))
        );
    }

    #[test]
    fn hdcp_and_signal_format_per_input() {
        let VideoMenuPart::Hdcp(hdcp) = page(
            VideoMenuPage::Hdcp,
            include_bytes!("../tests/data/xml/AVC-A10H-web-video-hdcp.xml"),
        ) else {
            panic!("not the HDCP page");
        };
        assert_eq!(hdcp.len(), 9);
        assert_eq!(hdcp[0].source, Some("SAT/CBL"));
        assert_eq!(hdcp[0].value, Some(HdcpMode::Auto));
        assert_eq!(hdcp[0].availability, Some(Availability::Available));
        assert!(!hdcp[0].custom_offered);
        // An input without an HDMI input is shown but blocked.
        let tv_audio = hdcp.iter().find(|s| s.source == Some("TV")).unwrap();
        assert_eq!(tv_audio.availability, Some(Availability::Blocked));

        let VideoMenuPart::SignalFormat(formats) = page(
            VideoMenuPage::SignalFormat,
            include_bytes!("../tests/data/xml/AVC-A10H-web-video-4k8k-per-input.xml"),
        ) else {
            panic!("not the signal format page");
        };
        assert_eq!(formats[0].value, Some(SignalFormat::Enhanced8k));
        assert!(formats[0].custom_offered);
        assert_eq!(formats[7].value, Some(SignalFormat::Standard));
        assert!(!formats[7].custom_offered);
    }

    #[test]
    fn another_page_and_html_are_errors() {
        assert!(
            VideoMenuPage::TvFormat
                .parse(b"<ScreenSaver>0</ScreenSaver>")
                .is_err()
        );
        assert!(
            VideoMenuPage::Hdcp
                .parse(b"<html><body>no</body></html>")
                .is_err()
        );
    }

    #[test]
    fn a_page_without_a_known_value_is_an_error() {
        // The right root, but nothing usable: it must not clear what is known.
        for (kind, body) in [
            (VideoMenuPage::OnScreenDisplay, &b"<OnScreenDisplay/>"[..]),
            (VideoMenuPage::TvFormat, b"<TVFormat>9</TVFormat>"),
            (VideoMenuPage::TvFormat, b"<TVFormat/>"),
            (VideoMenuPage::ScreenSaver, b"<ScreenSaver>x</ScreenSaver>"),
            (VideoMenuPage::HdmiUpscaler, b"<HDMIScaler>4</HDMIScaler>"),
            (VideoMenuPage::Hdcp, b"<HDCPSetup/>"),
            (
                VideoMenuPage::SignalFormat,
                b"<FourKEightKSignalFormatPerInput/>",
            ),
            (
                VideoMenuPage::Hdcp,
                b"<HDCPSetup><Source index=\"1\"><Value>9</Value></Source></HDCPSetup>",
            ),
        ] {
            assert!(kind.parse(body).is_err(), "{kind:?}");
        }
    }

    #[test]
    fn a_source_is_listed_once() {
        let VideoMenuPart::Hdcp(list) = page(
            VideoMenuPage::Hdcp,
            b"<HDCPSetup><Source index=\"3\"><Value>2</Value></Source>\
              <Source index=\"3\"><Value>3</Value></Source></HDCPSetup>",
        ) else {
            panic!("not the HDCP page");
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].value, Some(HdcpMode::V1_4));
        // The page does not say if the user can change it.
        assert_eq!(list[0].availability, None);
    }

    #[test]
    fn the_same_lists_again_change_nothing() {
        let mut menu = VideoMenu::default();
        let hdcp = || {
            page(
                VideoMenuPage::Hdcp,
                include_bytes!("../tests/data/xml/AVC-A10H-web-video-hdcp.xml"),
            )
        };
        assert!(menu.apply(hdcp()));
        assert!(!menu.apply(hdcp()));
    }

    #[test]
    fn applying_the_same_page_changes_nothing() {
        let mut menu = VideoMenu::default();
        let part = page(VideoMenuPage::ScreenSaver, b"<ScreenSaver>1</ScreenSaver>");
        assert!(menu.apply(part.clone()));
        assert!(!menu.apply(part));
        assert_eq!(menu.screen_saver, Some(true));
    }
}
