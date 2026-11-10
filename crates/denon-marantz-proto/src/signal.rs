//! Signal information: the video and audio signal that the receiver gets and
//! sends. Telnet reports it.

use std::fmt;

use crate::signal_pages::{AudioChannelsPage, VideoSignalPage};
use crate::values::{ColorSpace, PixelDepth};

/// The channels of an audio signal, as they are usually written: `5.1` or
/// `7.1.4`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChannelLayout {
    /// Channels around the listener, without the subwoofer.
    pub bed: u8,
    /// Subwoofer (LFE) channels.
    pub lfe: u8,
    /// Height and other overhead channels.
    pub immersive: u8,
}

impl ChannelLayout {
    /// Reads the channel text of the receiver. Each position of the text is a
    /// channel, and a `2` means that the channel is active. Returns `None` if
    /// no channel is active.
    #[must_use]
    pub fn from_status(text: &str) -> Option<Self> {
        Self::from_active(
            text.trim()
                .bytes()
                .enumerate()
                .filter(|&(_, status)| status == b'2')
                .map(|(index, _)| index),
        )
    }

    /// Counts the channels at these positions, the same positions as in
    /// [`ChannelLayout::from_status`]. Returns `None` if there are none.
    pub(crate) fn from_active(positions: impl Iterator<Item = usize>) -> Option<Self> {
        let mut layout = Self {
            bed: 0,
            lfe: 0,
            immersive: 0,
        };
        for index in positions {
            match index {
                0..=2 | 4..=8 => layout.bed += 1,
                3 => layout.lfe += 1,
                13 => layout.immersive += 2,
                _ => layout.immersive += 1,
            }
        }
        (layout.bed + layout.lfe + layout.immersive > 0).then_some(layout)
    }
}

impl fmt::Display for ChannelLayout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.bed, self.lfe)?;
        if self.immersive > 0 {
            write!(f, ".{}", self.immersive)?;
        }
        Ok(())
    }
}

/// A value for the input and one for the output of the receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InOut<T> {
    /// Value of the signal that the receiver gets.
    pub input: Option<T>,
    /// Value of the signal that the receiver sends to the TV.
    pub output: Option<T>,
}

// Not derived: a derive would need `T: Default`.
impl<T> Default for InOut<T> {
    fn default() -> Self {
        Self {
            input: None,
            output: None,
        }
    }
}

impl<T: PartialEq> InOut<T> {
    /// Sets the input or the output. Returns true if the value was different.
    pub(crate) fn set(&mut self, is_input: bool, value: Option<T>) -> bool {
        let slot = if is_input {
            &mut self.input
        } else {
            &mut self.output
        };
        if *slot == value {
            false
        } else {
            *slot = value;
            true
        }
    }
}

/// The video and audio signal. A value is `None` until the receiver reports
/// it, and when the receiver reports that there is none.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SignalState {
    /// Video resolution and frame rate, for example `4K50`.
    pub video: InOut<String>,
    /// HDR format.
    pub hdr: InOut<String>,
    /// Highest HDMI FRL rate.
    pub max_frl: InOut<String>,
    /// Bits for each color.
    pub pixel_depth: InOut<PixelDepth>,
    /// Color space.
    pub color_space: InOut<ColorSpace>,
    /// The signal uses the BT.2020 color range.
    pub bt2020: InOut<bool>,
    /// Highest resolution that the TV and the cables allow, as the receiver
    /// writes it, for example `NO` for no limit or `8G`.
    pub max_hdmi_resolution: Option<String>,
    /// Kind of audio signal that the receiver gets, for example `PCM`.
    pub audio_signal: Option<String>,
    /// Kind of audio that the receiver plays, for example `Stereo`.
    pub audio_sound: Option<String>,
    /// Audio sampling rate in kHz, for example `48` or `44.1`.
    pub sampling_rate: Option<String>,
    /// Active channels of the audio that the receiver gets and plays.
    pub channels: InOut<ChannelLayout>,
    /// Auto low latency mode is on. Only the web interface reports it.
    pub allm: Option<bool>,
    /// Quick frame transport is on. Only the web interface reports it.
    pub qft: Option<bool>,
}

impl SignalState {
    /// Applies the text of an `SS` event, for example `INFSIGRES I4K50`.
    /// Returns true if a value changed. Other events and text that is not a
    /// value change nothing.
    pub(crate) fn apply_ss(&mut self, parameter: &str) -> bool {
        if let Some(rest) = parameter.strip_prefix("INFSIGRES") {
            self.set_video(rest)
        } else if let Some(rest) = parameter.strip_prefix("INFSIGHDR") {
            set_side_text(&mut self.hdr, rest)
        } else if let Some(rest) = parameter.strip_prefix("INFSIGFRL") {
            set_side_text(&mut self.max_frl, rest)
        } else if let Some(rest) = parameter.strip_prefix("INFSIGPIX") {
            let Some((input, output)) = digit_pair(rest) else {
                return false;
            };
            let (input, output) = (PixelDepth::from_code(input), PixelDepth::from_code(output));
            self.pixel_depth.set(true, input) | self.pixel_depth.set(false, output)
        } else if let Some(rest) = parameter.strip_prefix("INFSIGCOS") {
            let Some((input, output)) = digit_pair(rest) else {
                return false;
            };
            let (input, output) = (ColorSpace::from_code(input), ColorSpace::from_code(output));
            self.color_space.set(true, input) | self.color_space.set(false, output)
        } else if let Some(rest) = parameter.strip_prefix("INFSIGECI") {
            // A digit that is not 0 means BT.2020.
            let Some((input, output)) = digit_pair(rest) else {
                return false;
            };
            self.bt2020.set(true, Some(input != "0")) | self.bt2020.set(false, Some(output != "0"))
        } else if let Some(rate) = parameter.strip_prefix("INFAISFSV") {
            set_text(&mut self.sampling_rate, sampling_rate(rate))
        } else {
            false
        }
    }

    /// Applies the text of an `OP` event, for example `INFINS 22000000`.
    /// Returns true if a value changed. Other events change nothing.
    pub(crate) fn apply_op(&mut self, parameter: &str) -> bool {
        if let Some(rest) = parameter.strip_prefix("INFINS") {
            self.channels.set(true, ChannelLayout::from_status(rest))
        } else if let Some(rest) = parameter.strip_prefix("INFASP") {
            self.channels.set(false, ChannelLayout::from_status(rest))
        } else {
            false
        }
    }

    /// Applies the text of a `SY` event, for example `SDA PCM`. Returns true
    /// if a value changed.
    pub(crate) fn apply_sy(&mut self, parameter: &str) -> bool {
        if let Some(rest) = parameter.strip_prefix("SDVIN") {
            self.video.set_text(true, video(rest))
        } else if let Some(rest) = parameter.strip_prefix("SDVOUT") {
            self.video.set_text(false, video(rest))
        } else if let Some(rest) = parameter.strip_prefix("SDA") {
            set_text(&mut self.audio_signal, audio_text(rest))
        } else if let Some(rest) = parameter.strip_prefix("SMI") {
            set_text(&mut self.audio_sound, audio_text(rest))
        } else if let Some(rest) = parameter.strip_prefix("HDMIDIAGMAXRES") {
            set_text(
                &mut self.max_hdmi_resolution,
                Some(rest.trim()).filter(|v| !v.is_empty()),
            )
        } else {
            false
        }
    }

    /// Applies the video page of the web interface. Returns true if a value
    /// changed. A side with no signal clears its values.
    pub(crate) fn apply_video_page(&mut self, page: &VideoSignalPage) -> bool {
        let mut changed = self.video.set_both(&page.resolution);
        changed |= self.hdr.set_both(&page.hdr);
        changed |= self.max_frl.set_both(&page.frl_rate);
        changed |= self.color_space.set(true, page.color_space.input)
            | self.color_space.set(false, page.color_space.output);
        changed |= self.pixel_depth.set(true, page.pixel_depth.input)
            | self.pixel_depth.set(false, page.pixel_depth.output);
        changed |= replace(&mut self.allm, page.allm);
        changed |= replace(&mut self.qft, page.qft);
        changed
    }

    /// Applies the channel page of the web interface. Returns true if a value
    /// changed.
    pub(crate) fn apply_channels_page(&mut self, page: &AudioChannelsPage) -> bool {
        self.channels.set(true, page.channels.input)
            | self.channels.set(false, page.channels.output)
    }

    fn set_video(&mut self, rest: &str) -> bool {
        let Some((side, value)) = split_side(rest) else {
            return false;
        };
        self.video.set_text(side, video(value))
    }
}

impl InOut<String> {
    /// Sets the input and the output. Returns true if a value changed.
    fn set_both(&mut self, new: &Self) -> bool {
        self.set_text(true, new.input.as_deref()) | self.set_text(false, new.output.as_deref())
    }

    pub(crate) fn set_text(&mut self, is_input: bool, value: Option<&str>) -> bool {
        let slot = if is_input {
            &mut self.input
        } else {
            &mut self.output
        };
        set_text(slot, value)
    }
}

/// The text after `I` or `O`. Some receivers write a space after the letter.
fn split_side(rest: &str) -> Option<(bool, &str)> {
    let rest = rest.trim_start();
    let is_input = match rest.as_bytes().first()? {
        b'I' => true,
        b'O' => false,
        _ => return None,
    };
    Some((is_input, &rest[1..]))
}

fn set_side_text(slot: &mut InOut<String>, rest: &str) -> bool {
    let Some((side, value)) = split_side(rest) else {
        return false;
    };
    slot.set_text(side, video(value))
}

/// A video value. `---` means that there is no signal.
fn video(text: &str) -> Option<&str> {
    Some(text.trim()).filter(|value| !matches!(*value, "" | "---"))
}

/// Two digits, one for the input and one for the output.
fn digit_pair(text: &str) -> Option<(&str, &str)> {
    let text = text.trim();
    (text.len() == 2 && text.bytes().all(|b| b.is_ascii_digit())).then(|| text.split_at(1))
}

/// Text of an audio event: it can be padded, and `Unknown` means none.
fn audio_text(text: &str) -> Option<&str> {
    Some(text.trim()).filter(|value| !matches!(*value, "" | "Unknown"))
}

/// The sampling rate in kHz as text: `441` is `44.1`, `176` is `176.4`, and
/// `48K` is `48`. `NON` and an empty text mean none.
fn sampling_rate(text: &str) -> Option<&str> {
    match text.trim() {
        "" | "NON" => None,
        "441" => Some("44.1"),
        "176" => Some("176.4"),
        value => Some(value.strip_suffix('K').unwrap_or(value)),
    }
}

/// Replaces a value. Returns true if it was different.
fn replace<T: PartialEq>(slot: &mut Option<T>, value: Option<T>) -> bool {
    if *slot == value {
        false
    } else {
        *slot = value;
        true
    }
}

/// Replaces the text. Returns true if it was different. The buffer is reused.
fn set_text(slot: &mut Option<String>, value: Option<&str>) -> bool {
    match (slot.as_mut(), value) {
        (Some(current), Some(value)) if current == value => false,
        (Some(current), Some(value)) => {
            current.clear();
            current.push_str(value);
            true
        }
        (None, Some(value)) => {
            *slot = Some(value.to_owned());
            true
        }
        (Some(_), None) => {
            *slot = None;
            true
        }
        (None, None) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ss(signal: &mut SignalState, text: &str) -> bool {
        signal.apply_ss(text)
    }

    #[test]
    fn video_events() {
        let mut signal = SignalState::default();
        assert!(ss(&mut signal, "INFSIGRES I4K50"));
        assert!(ss(&mut signal, "INFSIGRES O1080p60"));
        assert!(!ss(&mut signal, "INFSIGRES I4K50"));
        assert_eq!(signal.video.input.as_deref(), Some("4K50"));
        assert_eq!(signal.video.output.as_deref(), Some("1080p60"));
        // Some receivers write a space after the letter.
        assert!(ss(&mut signal, "INFSIGRES I 4K60"));
        assert_eq!(signal.video.input.as_deref(), Some("4K60"));
        // No signal.
        assert!(ss(&mut signal, "INFSIGRES I---"));
        assert_eq!(signal.video.input, None);
        // The other event has the same values.
        assert!(signal.apply_sy("SDVIN 4K50"));
        assert!(signal.apply_sy("SDVOUT 4K24"));
        assert_eq!(signal.video.input.as_deref(), Some("4K50"));
        assert_eq!(signal.video.output.as_deref(), Some("4K24"));
        assert!(!signal.apply_sy("SDVIN 4K50"));
    }

    #[test]
    fn hdr_and_frl_events() {
        let mut signal = SignalState::default();
        assert!(ss(&mut signal, "INFSIGHDR IHDR10"));
        assert!(ss(&mut signal, "INFSIGHDR OHDR10"));
        assert!(ss(&mut signal, "INFSIGHDR O---"));
        assert_eq!(signal.hdr.input.as_deref(), Some("HDR10"));
        assert_eq!(signal.hdr.output, None);
        assert!(ss(&mut signal, "INFSIGFRL I 24G"));
        assert!(ss(&mut signal, "INFSIGFRL O 24G"));
        assert!(ss(&mut signal, "INFSIGFRL O---"));
        assert_eq!(signal.max_frl.input.as_deref(), Some("24G"));
        assert_eq!(signal.max_frl.output, None);
    }

    #[test]
    fn pixel_depth_color_space_and_bt2020() {
        let mut signal = SignalState::default();
        assert!(ss(&mut signal, "INFSIGPIX 12"));
        assert_eq!(signal.pixel_depth.input, Some(PixelDepth::Bits8));
        assert_eq!(signal.pixel_depth.output, Some(PixelDepth::Bits10));
        assert!(ss(&mut signal, "INFSIGCOS 28"));
        assert_eq!(signal.color_space.input, Some(ColorSpace::Yuv444));
        assert_eq!(signal.color_space.output, Some(ColorSpace::Yuv420));
        assert!(ss(&mut signal, "INFSIGECI 01"));
        assert_eq!(signal.bt2020.input, Some(false));
        assert_eq!(signal.bt2020.output, Some(true));
        assert!(!ss(&mut signal, "INFSIGECI 01"));
        // A digit that is not known makes the value unknown.
        assert!(ss(&mut signal, "INFSIGCOS 47"));
        assert_eq!(signal.color_space.input, None);
        assert_eq!(signal.color_space.output, None);
        // Text that is not two digits changes nothing.
        let before = signal.clone();
        for text in [
            "INFSIGPIX",
            "INFSIGPIX 1",
            "INFSIGPIX 123",
            "INFSIGPIX ab",
            "INFSIGECI 0x",
        ] {
            assert!(!ss(&mut signal, text), "{text}");
        }
        assert_eq!(signal, before);
    }

    #[test]
    fn audio_events() {
        let mut signal = SignalState::default();
        assert!(signal.apply_sy("SDA PCM                  "));
        assert!(signal.apply_sy("SMI Stereo"));
        assert_eq!(signal.audio_signal.as_deref(), Some("PCM"));
        assert_eq!(signal.audio_sound.as_deref(), Some("Stereo"));
        assert!(!signal.apply_sy("SDA PCM"));
        assert!(signal.apply_sy("SDA Unknown"));
        assert_eq!(signal.audio_signal, None);
        assert!(signal.apply_sy("HDMIDIAGMAXRES NO"));
        assert_eq!(signal.max_hdmi_resolution.as_deref(), Some("NO"));
    }

    #[test]
    fn sampling_rates() {
        let mut signal = SignalState::default();
        for (text, expected) in [
            ("INFAISFSV 48K", Some("48")),
            ("INFAISFSV 441", Some("44.1")),
            ("INFAISFSV 176", Some("176.4")),
            ("INFAISFSV 192", Some("192")),
            ("INFAISFSV 96K", Some("96")),
            ("INFAISFSV NON", None),
            ("INFAISFSV ", None),
        ] {
            ss(&mut signal, text);
            assert_eq!(signal.sampling_rate.as_deref(), expected, "{text}");
        }
    }

    #[test]
    fn channel_layouts() {
        let layout = |text: &str| ChannelLayout::from_status(text).map(|l| l.to_string());
        assert_eq!(layout("22111111111111000000").as_deref(), Some("2.0"));
        assert_eq!(layout("22222222000000000000").as_deref(), Some("7.1"));
        assert_eq!(layout("22222222022000000000").as_deref(), Some("7.1.2"));
        assert_eq!(layout("22222222000022000000").as_deref(), Some("7.1.3"));
        assert_eq!(layout("22000000000002").as_deref(), Some("2.0.2"));
        assert_eq!(layout("00000000"), None);
        assert_eq!(layout(""), None);
    }

    #[test]
    fn channel_events() {
        let mut signal = SignalState::default();
        assert!(signal.apply_op("INFINS 22000000000000000000"));
        assert!(signal.apply_op("INFASP 22222222000000000000"));
        assert!(!signal.apply_op("INFASP 22222222000000000000"));
        assert_eq!(
            signal.channels.input.map(|l| l.to_string()).as_deref(),
            Some("2.0")
        );
        assert_eq!(
            signal.channels.output.map(|l| l.to_string()).as_deref(),
            Some("7.1")
        );
        // No channel is active.
        assert!(signal.apply_op("INFINS 00000000"));
        assert_eq!(signal.channels.input, None);
        for text in ["", "ALSSET ON", "INFAISFSV 48K"] {
            assert!(!signal.apply_op(text), "{text}");
        }
    }

    #[test]
    fn other_events_change_nothing() {
        let mut signal = SignalState::default();
        for text in ["", "TTR OFF", "INFAISSIG 02", "INFSIGRES", "INFSIGRES X4K"] {
            assert!(!ss(&mut signal, text), "{text}");
        }
        for text in ["", "SVOUT", "HDMIDIAGERR NO", "SMX"] {
            assert!(!signal.apply_sy(text), "{text}");
        }
    }
}
