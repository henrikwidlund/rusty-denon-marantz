//! Signal information from the web interface: the video signal and the
//! channels of the audio signal.
//!
//! Telnet reports the same values, but only when it is asked. The pages answer
//! at once, so a client that starts up reads them first. The pages do not send
//! changes.

use crate::error::Error;
use crate::reading::{Reading, Sides};
use crate::signal::ChannelLayout;
use crate::speaker_tables::{number, root};
use crate::values::{ColorSpace, PixelDepth};
use crate::xml::Node;

/// Path of the page with the video signal.
pub const VIDEO_SIGNAL_PATH: &str = "/ajax/info/get_config?type=3";

/// Path of the page with the channels of the audio signal.
pub const AUDIO_CHANNELS_PATH: &str = "/ajax/info/get_config?type=2";

/// The video signal that the receiver gets and sends.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VideoSignalPage {
    /// Resolution and frame rate, for example `4K50`.
    pub resolution: Sides<String>,
    /// HDR format.
    pub hdr: Sides<String>,
    /// HDMI FRL rate.
    pub frl_rate: Sides<String>,
    /// Color space.
    pub color_space: Sides<ColorSpace>,
    /// The signal uses the BT.2020 color range. Pages without a BT.2020
    /// signal do not have the `Colorimetry` element.
    pub bt2020: Sides<bool>,
    /// Bits for each color.
    pub pixel_depth: Sides<PixelDepth>,
    /// Auto low latency mode is on.
    pub allm: Reading<bool>,
    /// Quick frame transport is on.
    pub qft: Reading<bool>,
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
        let known = [
            "Resolution",
            "HDR",
            "FRLRate",
            "ColorSpace",
            "Colorimetry",
            "PixelDepth",
        ];
        if !known.iter().any(|name| node.child(name).is_some()) {
            return Err(Error::UnexpectedDocument { expected: "Video" });
        }
        let text = |name: &str| {
            let side = node.child(name).map(|child| child.node());
            // A value with no content has no signal. One with other content
            // is not known, and keeps the old value.
            let empty = side.is_some_and(|side| {
                side.children().next().is_none()
                    && side.text().is_none_or(|text| text.trim().is_empty())
            });
            let read = |side_name: &str| -> Reading<String> {
                if empty {
                    return Reading::Empty;
                }
                let Some(element) = side.and_then(|side| side.child(side_name)) else {
                    return Reading::Absent;
                };
                let value = element.node().text();
                Reading::from_option(value.as_deref().and_then(signal_text).map(str::to_owned))
            };
            Sides {
                input: read("Input"),
                output: read("Output"),
            }
        };
        let color_space = text("ColorSpace");
        // `BT.2020` is true, other texts are false.
        let colorimetry = text("Colorimetry");
        let pixel_depth = text("PixelDepth");
        Ok(Self {
            resolution: text("Resolution"),
            hdr: text("HDR"),
            frl_rate: text("FRLRate"),
            bt2020: Sides {
                input: colorimetry.input.convert(|text| Some(text == "BT.2020")),
                output: colorimetry.output.convert(|text| Some(text == "BT.2020")),
            },
            color_space: Sides {
                input: color_space.input.convert(color_space_of),
                output: color_space.output.convert(color_space_of),
            },
            pixel_depth: Sides {
                input: pixel_depth.input.convert(pixel_depth_of),
                output: pixel_depth.output.convert(pixel_depth_of),
            },
            allm: on(node, "ALLM"),
            qft: on(node, "QFT"),
        })
    }
}

/// The channels of the audio signal that the receiver gets and plays.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioChannelsPage {
    /// Channels of the signal and the speakers that play it. A side with no
    /// active channel is [`Reading::Empty`].
    pub channels: Sides<ChannelLayout>,
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
            let Some(element) = node.child(name) else {
                return Reading::Absent;
            };
            // An element with only a text has no channels.
            let Some(list) = element.node().child("List") else {
                return Reading::Empty;
            };
            let active = list
                .node()
                .children()
                .filter(|c| c.is("Speaker"))
                .filter_map(|speaker| {
                    // The receiver writes 2 for a channel that is active.
                    let is_active = speaker.node().text()?.trim() == "2";
                    let index = speaker.attribute("index")?.trim().parse().ok()?;
                    is_active.then_some(index)
                });
            Reading::from_option(ChannelLayout::from_active(active, false))
        };
        Ok(Self {
            channels: Sides {
                input: layout("InputSignal"),
                output: layout("ActiveSpeaker"),
            },
        })
    }
}

/// `1` is on and `2` is off, as in the other pages. An element with another
/// text is [`Reading::Empty`].
fn on(node: Node<'_>, name: &str) -> Reading<bool> {
    if node.child(name).is_none() {
        return Reading::Absent;
    }
    Reading::from_option(match number::<u8>(node, name) {
        Some(1) => Some(true),
        Some(2) => Some(false),
        _ => None,
    })
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
    const HDR10: &[u8] = include_bytes!("../tests/data/xml/AVC-A10H-web-video-signal-hdr10.xml");
    const NO_VIDEO: &[u8] = include_bytes!("../tests/data/xml/AVC-A10H-web-video-no-signal.xml");
    const AUDIO: &[u8] = include_bytes!("../tests/data/xml/AVC-A10H-web-audio-channels.xml");

    use Reading::{Absent, Empty, Value};

    fn text(value: &str) -> Reading<String> {
        Value(value.to_owned())
    }

    #[test]
    fn video_page_with_a_signal() {
        let page = VideoSignalPage::parse(VIDEO).unwrap();
        assert_eq!(page.resolution.input, text("4K50"));
        assert_eq!(page.resolution.output, text("4K50"));
        // `---` is a value with no signal.
        assert_eq!(
            page.hdr,
            Sides {
                input: Empty,
                output: Empty
            }
        );
        assert_eq!(
            page.frl_rate,
            Sides {
                input: Empty,
                output: Empty
            }
        );
        assert_eq!(page.color_space.input, Value(ColorSpace::Yuv444));
        assert_eq!(page.pixel_depth.output, Value(PixelDepth::Bits8));
        assert_eq!(page.allm, Value(false));
        assert_eq!(page.qft, Value(false));
    }

    #[test]
    fn video_page_with_an_hdr10_signal() {
        let page = VideoSignalPage::parse(HDR10).unwrap();
        assert_eq!(page.hdr.input, text("HDR10"));
        assert_eq!(page.hdr.output, text("HDR10"));
        assert_eq!(page.color_space.input, Value(ColorSpace::Yuv422));
        assert_eq!(page.color_space.output, Value(ColorSpace::Yuv422));
        assert_eq!(page.bt2020.input, Value(true));
        assert_eq!(page.bt2020.output, Value(true));
        assert_eq!(page.pixel_depth.input, Value(PixelDepth::Bits12));
        // The SDR page has no colorimetry.
        assert_eq!(
            VideoSignalPage::parse(VIDEO).unwrap().bt2020,
            Sides::default()
        );
        let other = VideoSignalPage::parse(
            b"<Video><Colorimetry><Input>BT.709</Input></Colorimetry></Video>",
        )
        .unwrap();
        assert_eq!(other.bt2020.input, Value(false));
    }

    #[test]
    fn video_page_without_a_signal_has_no_values() {
        let page = VideoSignalPage::parse(NO_VIDEO).unwrap();
        assert_eq!(
            page.resolution,
            Sides {
                input: Empty,
                output: Empty
            }
        );
        assert_eq!(
            page.color_space,
            Sides {
                input: Empty,
                output: Empty
            }
        );
        assert_eq!(
            page.pixel_depth,
            Sides {
                input: Empty,
                output: Empty
            }
        );
        assert_eq!(page.allm, Value(false));
        assert_eq!(page.resolution.input.as_value(), None);
    }

    #[test]
    fn audio_page_counts_the_active_channels() {
        let page = AudioChannelsPage::parse(AUDIO).unwrap();
        let input = page.channels.input.as_value().unwrap();
        assert_eq!(input.to_string(), "2.0");
        let output = page.channels.output.as_value().unwrap();
        assert_eq!(output.to_string(), "2.1");
    }

    #[test]
    fn an_atmos_page_has_text_for_the_input_and_one_index_for_each_output_channel() {
        let page = AudioChannelsPage::parse(include_bytes!(
            "../tests/data/xml/AVC-A10H-web-audio-channels-atmos.xml"
        ))
        .unwrap();
        // The input of an object based stream is only a text, not channels.
        assert_eq!(page.channels.input, Empty);
        // Index 13 of the page is one channel: the receiver plays 7.1.4.
        assert_eq!(
            page.channels.output.as_value().unwrap().to_string(),
            "7.1.4"
        );
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
        let page = VideoSignalPage::parse(
            b"<Video><Resolution><Input/><Output>---</Output></Resolution></Video>",
        )
        .unwrap();
        assert_eq!(
            page.resolution,
            Sides {
                input: Empty,
                output: Empty
            }
        );
        assert!(AudioChannelsPage::parse(b"<Audio><InputSignal/></Audio>").is_ok());
    }

    #[test]
    fn a_value_with_no_content_has_no_signal() {
        let page = VideoSignalPage::parse(b"<Video><Resolution/><HDR></HDR></Video>").unwrap();
        let empty = Sides {
            input: Empty,
            output: Empty,
        };
        assert_eq!(page.resolution, empty);
        assert_eq!(page.hdr, empty);
        assert_eq!(page.frl_rate, Sides::default());
        // Content that is not known keeps the old value.
        let page = VideoSignalPage::parse(
            b"<Video><Resolution>4K50</Resolution><HDR><Level>1</Level></HDR><FRLRate/></Video>",
        )
        .unwrap();
        assert_eq!(page.resolution, Sides::default());
        assert_eq!(page.hdr, Sides::default());
        assert_eq!(page.frl_rate, empty);
    }

    #[test]
    fn a_partial_page_leaves_the_other_values_out() {
        let page = VideoSignalPage::parse(
            b"<Video><Resolution><Input>1080p60</Input></Resolution></Video>",
        )
        .unwrap();
        assert_eq!(page.resolution.input, text("1080p60"));
        assert_eq!(page.resolution.output, Absent);
        assert_eq!(page.hdr, Sides::default());
        assert_eq!(page.color_space, Sides::default());
        assert_eq!(page.allm, Absent);
        // An element with a text that is not known has no value.
        let page = VideoSignalPage::parse(
            b"<Video><ALLM>9</ALLM><ColorSpace><Input>YCbCr 4:4:0</Input></ColorSpace></Video>",
        )
        .unwrap();
        assert_eq!(page.allm, Empty);
        assert_eq!(page.color_space.input, Empty);
        assert_eq!(page.color_space.output, Absent);
        let page = AudioChannelsPage::parse(
            b"<Audio><InputSignal><List><Speaker index=\"0\">2</Speaker></List></InputSignal></Audio>",
        )
        .unwrap();
        assert_eq!(page.channels.input.as_value().unwrap().to_string(), "1.0");
        assert_eq!(page.channels.output, Absent);
    }

    #[test]
    fn many_or_repeated_channels_do_not_overflow() {
        let mut page = String::from("<Audio><InputSignal><List>");
        for _ in 0..300 {
            page.push_str("<Speaker index=\"20\">2</Speaker>");
        }
        page.push_str("<Speaker index=\"70000\">2</Speaker></List></InputSignal></Audio>");
        let page = AudioChannelsPage::parse(page.as_bytes()).unwrap();
        // One position counts once, and an unknown position is ignored.
        assert_eq!(page.channels.input.as_value().unwrap().to_string(), "0.0.1");
    }
}
