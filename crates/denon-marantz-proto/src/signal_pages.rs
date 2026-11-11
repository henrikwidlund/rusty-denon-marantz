//! Signal information from the web interface: the video signal and the
//! channels of the audio signal.
//!
//! Telnet reports the same values, but only when it is asked. The pages answer
//! at once, so a client that starts up reads them first. The pages do not send
//! changes.

use crate::error::Error;
use crate::signal::{ChannelLayout, InOut};
use crate::speaker_tables::{number, root};
use crate::values::{ColorSpace, PixelDepth};
use crate::xml::Node;

/// Path of the page with the video signal.
pub const VIDEO_SIGNAL_PATH: &str = "/ajax/info/get_config?type=3";

/// Path of the page with the channels of the audio signal.
pub const AUDIO_CHANNELS_PATH: &str = "/ajax/info/get_config?type=2";

/// The video signal that the receiver gets and sends. A value is `None` if
/// there is no signal or the page does not have it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VideoSignalPage {
    /// Resolution and frame rate, for example `4K50`.
    pub resolution: InOut<String>,
    /// HDR format.
    pub hdr: InOut<String>,
    /// HDMI FRL rate.
    pub frl_rate: InOut<String>,
    /// Color space.
    pub color_space: InOut<ColorSpace>,
    /// Bits for each color.
    pub pixel_depth: InOut<PixelDepth>,
    /// Auto low latency mode is on.
    pub allm: Option<bool>,
    /// Quick frame transport is on.
    pub qft: Option<bool>,
}

impl VideoSignalPage {
    /// Reads the page.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML, is an HTML page, or is
    /// not the video page.
    pub fn parse(body: &[u8]) -> Result<Self, Error> {
        let root = root(body)?;
        if !root.is("Video") {
            return Err(Error::UnexpectedDocument { expected: "Video" });
        }
        let node = root.node();
        // A page without these elements is not a signal page, and must not
        // clear the state.
        let known = ["Resolution", "HDR", "FRLRate", "ColorSpace", "PixelDepth"];
        if !known.iter().any(|name| node.child(name).is_some()) {
            return Err(Error::UnexpectedDocument { expected: "Video" });
        }
        let text = |name: &str| {
            let side = node.child(name).map(|child| child.node());
            let read = |side_name: &str| {
                let value = side?.child(side_name)?.node().text()?;
                signal_text(&value).map(str::to_owned)
            };
            InOut {
                input: read("Input"),
                output: read("Output"),
            }
        };
        let color_space = text("ColorSpace");
        let pixel_depth = text("PixelDepth");
        Ok(Self {
            resolution: text("Resolution"),
            hdr: text("HDR"),
            frl_rate: text("FRLRate"),
            color_space: InOut {
                input: color_space.input.as_deref().and_then(color_space_of),
                output: color_space.output.as_deref().and_then(color_space_of),
            },
            pixel_depth: InOut {
                input: pixel_depth.input.as_deref().and_then(pixel_depth_of),
                output: pixel_depth.output.as_deref().and_then(pixel_depth_of),
            },
            allm: on(node, "ALLM"),
            qft: on(node, "QFT"),
        })
    }
}

/// The channels of the audio signal that the receiver gets and plays.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioChannelsPage {
    /// Channels of the signal and the speakers that play it. `None` if no
    /// channel is active.
    pub channels: InOut<ChannelLayout>,
}

impl AudioChannelsPage {
    /// Reads the page.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML, is an HTML page, or is
    /// not the channels page.
    pub fn parse(body: &[u8]) -> Result<Self, Error> {
        let root = root(body)?;
        if !root.is("Audio") {
            return Err(Error::UnexpectedDocument { expected: "Audio" });
        }
        let node = root.node();
        if ["InputSignal", "ActiveSpeaker"]
            .iter()
            .all(|name| node.child(name).is_none())
        {
            return Err(Error::UnexpectedDocument { expected: "Audio" });
        }
        let layout = |name: &str| {
            let list = node.child(name)?.node().child("List")?.node();
            let active = list
                .children()
                .filter(|c| c.is("Speaker"))
                .filter_map(|speaker| {
                    // The receiver writes 2 for a channel that is active.
                    let is_active = speaker.node().text()?.trim() == "2";
                    let index = speaker.attribute("index")?.trim().parse().ok()?;
                    is_active.then_some(index)
                });
            ChannelLayout::from_active(active, false)
        };
        Ok(Self {
            channels: InOut {
                input: layout("InputSignal"),
                output: layout("ActiveSpeaker"),
            },
        })
    }
}

/// `1` is on and `2` is off, as in the other pages.
fn on(node: Node<'_>, name: &str) -> Option<bool> {
    match number::<u8>(node, name)? {
        1 => Some(true),
        2 => Some(false),
        _ => None,
    }
}

/// A value of the video page. `---` and an empty text mean that there is no
/// signal.
fn signal_text(text: &str) -> Option<&str> {
    Some(text.trim()).filter(|value| !matches!(*value, "" | "---"))
}

/// `YCbCr 4:4:4` and `RGB`.
fn color_space_of(text: &str) -> Option<ColorSpace> {
    match text.strip_prefix("YCbCr ").unwrap_or(text) {
        "4:2:2" => Some(ColorSpace::Yuv422),
        "4:4:4" => Some(ColorSpace::Yuv444),
        "RGB" => Some(ColorSpace::Rgb),
        "4:2:0" => Some(ColorSpace::Yuv420),
        _ => None,
    }
}

/// `8bits`, `10bits` and `12bits`.
fn pixel_depth_of(text: &str) -> Option<PixelDepth> {
    match text {
        "8bits" => Some(PixelDepth::Bits8),
        "10bits" => Some(PixelDepth::Bits10),
        "12bits" => Some(PixelDepth::Bits12),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO: &[u8] = include_bytes!("../tests/data/xml/AVC-A10H-web-video-signal.xml");
    const NO_VIDEO: &[u8] = include_bytes!("../tests/data/xml/AVC-A10H-web-video-no-signal.xml");
    const AUDIO: &[u8] = include_bytes!("../tests/data/xml/AVC-A10H-web-audio-channels.xml");

    #[test]
    fn video_page_with_a_signal() {
        let page = VideoSignalPage::parse(VIDEO).unwrap();
        assert_eq!(page.resolution.input.as_deref(), Some("4K50"));
        assert_eq!(page.resolution.output.as_deref(), Some("4K50"));
        assert_eq!(page.hdr, InOut::default());
        assert_eq!(page.frl_rate, InOut::default());
        assert_eq!(page.color_space.input, Some(ColorSpace::Yuv444));
        assert_eq!(page.pixel_depth.output, Some(PixelDepth::Bits8));
        assert_eq!(page.allm, Some(false));
        assert_eq!(page.qft, Some(false));
    }

    #[test]
    fn video_page_without_a_signal_has_no_values() {
        let page = VideoSignalPage::parse(NO_VIDEO).unwrap();
        assert_eq!(page.resolution, InOut::default());
        assert_eq!(page.color_space, InOut::default());
        assert_eq!(page.pixel_depth, InOut::default());
        assert_eq!(page.allm, Some(false));
    }

    #[test]
    fn audio_page_counts_the_active_channels() {
        let page = AudioChannelsPage::parse(AUDIO).unwrap();
        let input = page.channels.input.unwrap();
        assert_eq!(input.to_string(), "2.0");
        let output = page.channels.output.unwrap();
        assert_eq!(output.to_string(), "2.1");
    }

    #[test]
    fn an_atmos_page_has_text_for_the_input_and_one_index_for_each_output_channel() {
        let page = AudioChannelsPage::parse(include_bytes!(
            "../tests/data/xml/AVC-A10H-web-audio-channels-atmos.xml"
        ))
        .unwrap();
        // The input of an object based stream is only a text, not channels.
        assert_eq!(page.channels.input, None);
        // Index 13 of the page is one channel: the receiver plays 7.1.4.
        assert_eq!(page.channels.output.unwrap().to_string(), "7.1.4");
    }

    #[test]
    fn unknown_texts_are_none() {
        assert_eq!(color_space_of("YCbCr 4:4:0"), None);
        assert_eq!(color_space_of("RGB"), Some(ColorSpace::Rgb));
        assert_eq!(pixel_depth_of("16bits"), None);
    }

    #[test]
    fn html_is_an_error() {
        assert!(VideoSignalPage::parse(b"<html><body>no</body></html>").is_err());
        assert!(AudioChannelsPage::parse(b"<html><body>no</body></html>").is_err());
    }

    #[test]
    fn another_document_is_not_an_empty_page() {
        // Valid XML of another page must not clear the state.
        assert_eq!(
            VideoSignalPage::parse(b"<Error/>").unwrap_err(),
            Error::UnexpectedDocument { expected: "Video" }
        );
        assert!(AudioChannelsPage::parse(VIDEO).is_err());
        assert!(VideoSignalPage::parse(AUDIO).is_err());
    }

    #[test]
    fn a_page_without_content_is_an_error() {
        assert!(VideoSignalPage::parse(b"<Video/>").is_err());
        assert!(AudioChannelsPage::parse(b"<Audio/>").is_err());
        // A page with no signal still has its elements.
        let page = VideoSignalPage::parse(b"<Video><Resolution/></Video>").unwrap();
        assert_eq!(page.resolution.input, None);
        assert!(AudioChannelsPage::parse(b"<Audio><InputSignal/></Audio>").is_ok());
    }

    #[test]
    fn many_or_repeated_channels_do_not_overflow() {
        let mut page = String::from("<Audio><InputSignal><List>");
        for _ in 0..300 {
            page.push_str("<Speaker index=\"20\">2</Speaker>");
        }
        page.push_str("<Speaker index=\"70000\">2</Speaker></List></InputSignal></Audio>");
        let layout = AudioChannelsPage::parse(page.as_bytes())
            .unwrap()
            .channels
            .input
            .unwrap();
        // One position counts once, and an unknown position is ignored.
        assert_eq!(layout.to_string(), "0.0.1");
    }
}
