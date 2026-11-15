//! Video settings that the receiver reports on telnet as `VS` events.

use core::fmt::Write as _;

use crate::values::{
    AspectRatio, HdmiAudioDecode, HdmiOutput, VideoProcessingMode, VideoResolution,
};

/// Setting of the HDMI upscaler: off, or on with a resolution. The receiver
/// calls the automatic resolution "on".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HdmiUpscaler {
    /// The upscaler is off.
    Off,
    /// The upscaler is on and makes this resolution.
    Resolution(VideoResolution),
}

impl HdmiUpscaler {
    /// Code used by the receiver.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Resolution(resolution) => resolution.code(),
        }
    }

    /// Parses a code used by the receiver.
    #[must_use]
    pub fn from_code(code: &str) -> Option<Self> {
        if code == "OFF" {
            Some(Self::Off)
        } else {
            VideoResolution::from_code(code).map(Self::Resolution)
        }
    }
}

/// The video settings of a receiver. A value is `None` until the receiver
/// reports it. A receiver only reports the settings that it has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct VideoSettings {
    /// Video processing mode.
    pub processing_mode: Option<VideoProcessingMode>,
    /// HDMI monitor output.
    pub hdmi_output: Option<HdmiOutput>,
    /// Where the HDMI audio is decoded.
    pub hdmi_audio: Option<HdmiAudioDecode>,
    /// Aspect ratio. Older receivers only.
    pub aspect_ratio: Option<AspectRatio>,
    /// Vertical stretch is on. Older receivers only.
    pub vertical_stretch: Option<bool>,
    /// Resolution of the analog to HDMI conversion. Older receivers only.
    pub resolution: Option<VideoResolution>,
    /// HDMI upscaler.
    pub hdmi_upscaler: Option<HdmiUpscaler>,
}

impl VideoSettings {
    /// Applies the text of a `VS` event, for example `VPMBYP`. Returns `None`
    /// if the text is not a video setting that this library knows, and
    /// whether the value changed otherwise.
    pub(crate) fn apply(&mut self, parameter: &str) -> Option<bool> {
        let parameter = parameter.trim();
        // `SCH` before `SC`, because the first is also a start of the second.
        if let Some(code) = parameter.strip_prefix("VPM") {
            put(
                &mut self.processing_mode,
                VideoProcessingMode::from_code(code),
            )
        } else if parameter.starts_with("MONI") {
            put(&mut self.hdmi_output, HdmiOutput::from_telnet(parameter))
        } else if let Some(code) = parameter.strip_prefix("AUDIO") {
            put(
                &mut self.hdmi_audio,
                HdmiAudioDecode::from_code(code.trim()),
            )
        } else if let Some(code) = parameter.strip_prefix("ASP") {
            put(&mut self.aspect_ratio, AspectRatio::from_code(code))
        } else if let Some(value) = parameter.strip_prefix("VST") {
            put(&mut self.vertical_stretch, on_off(value.trim()))
        } else if let Some(code) = parameter.strip_prefix("SCH") {
            put(&mut self.hdmi_upscaler, HdmiUpscaler::from_code(code))
        } else if let Some(code) = parameter.strip_prefix("SC") {
            put(&mut self.resolution, VideoResolution::from_code(code))
        } else {
            None
        }
    }
}

fn put<T: PartialEq>(slot: &mut Option<T>, value: Option<T>) -> Option<bool> {
    let value = value?;
    let changed = slot.as_ref() != Some(&value);
    *slot = Some(value);
    Some(changed)
}

fn on_off(text: &str) -> Option<bool> {
    match text {
        "ON" => Some(true),
        "OFF" => Some(false),
        _ => None,
    }
}

/// A video setting with the value to set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum VideoSettingValue {
    /// Sets the video processing mode.
    ProcessingMode(VideoProcessingMode),
    /// Sets the HDMI monitor output.
    HdmiOutput(HdmiOutput),
    /// Sets where the HDMI audio is decoded.
    HdmiAudio(HdmiAudioDecode),
    /// Sets the aspect ratio.
    AspectRatio(AspectRatio),
    /// Turns the vertical stretch on (`true`) or off (`false`).
    VerticalStretch(bool),
    /// Sets the resolution of the analog to HDMI conversion.
    Resolution(VideoResolution),
    /// Sets the HDMI upscaler.
    HdmiUpscaler(HdmiUpscaler),
}

impl VideoSettingValue {
    /// Appends the command text, without the line end.
    pub(crate) fn write(self, out: &mut String) {
        match self {
            Self::ProcessingMode(mode) => {
                out.push_str("VSVPM");
                out.push_str(mode.code());
            }
            Self::HdmiOutput(output) => {
                out.push_str("VS");
                out.push_str(output.telnet_code());
            }
            Self::HdmiAudio(decode) => {
                out.push_str("VSAUDIO ");
                out.push_str(decode.code());
            }
            Self::AspectRatio(aspect) => {
                out.push_str("VSASP");
                out.push_str(aspect.code());
            }
            Self::VerticalStretch(on) => {
                let _ = write!(out, "VSVST {}", if on { "ON" } else { "OFF" });
            }
            Self::Resolution(resolution) => {
                out.push_str("VSSC");
                out.push_str(resolution.code());
            }
            Self::HdmiUpscaler(upscaler) => {
                out.push_str("VSSCH");
                out.push_str(upscaler.code());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_fill_the_values() {
        let mut settings = VideoSettings::default();
        assert_eq!(settings.apply("VPMBYP"), Some(true));
        assert_eq!(settings.apply("VPMBYP"), Some(false));
        assert_eq!(settings.apply("MONI1"), Some(true));
        assert_eq!(settings.apply("AUDIO AMP"), Some(true));
        assert_eq!(settings.apply("ASPFUL"), Some(true));
        assert_eq!(settings.apply("VST ON"), Some(true));
        assert_eq!(settings.apply("VSTOFF"), Some(true));
        assert_eq!(settings.apply("SC4KF"), Some(true));
        assert_eq!(settings.apply("SCHOFF"), Some(true));
        assert_eq!(settings.processing_mode, Some(VideoProcessingMode::Bypass));
        assert_eq!(settings.hdmi_output, Some(HdmiOutput::Hdmi1));
        assert_eq!(settings.hdmi_audio, Some(HdmiAudioDecode::Amp));
        assert_eq!(settings.aspect_ratio, Some(AspectRatio::Full));
        assert_eq!(settings.vertical_stretch, Some(false));
        assert_eq!(settings.resolution, Some(VideoResolution::K4Fast));
        assert_eq!(settings.hdmi_upscaler, Some(HdmiUpscaler::Off));
    }

    #[test]
    fn upscaler_and_resolution_are_not_mixed_up() {
        let mut settings = VideoSettings::default();
        assert_eq!(settings.apply("SCHAUTO"), Some(true));
        assert_eq!(settings.resolution, None);
        assert_eq!(
            settings.hdmi_upscaler,
            Some(HdmiUpscaler::Resolution(VideoResolution::Auto))
        );
        assert_eq!(settings.apply("SC10P24"), Some(true));
        assert_eq!(settings.resolution, Some(VideoResolution::P1080At24));
    }

    #[test]
    fn unknown_text_is_not_a_setting() {
        let mut settings = VideoSettings::default();
        for text in [
            "", "XYZ", "VPM", "VPMXX", "MONI9", "AUDIO XX", "SCHXX", "VST ?", "ASP ?",
        ] {
            assert_eq!(settings.apply(text), None, "{text:?}");
        }
        assert_eq!(settings, VideoSettings::default());
    }

    #[test]
    fn commands() {
        let text = |value: VideoSettingValue| {
            let mut out = String::new();
            value.write(&mut out);
            out
        };
        let cases = [
            (
                VideoSettingValue::ProcessingMode(VideoProcessingMode::Movie),
                "VSVPMMOVI",
            ),
            (
                VideoSettingValue::HdmiOutput(HdmiOutput::Auto),
                "VSMONIAUTO",
            ),
            (
                VideoSettingValue::HdmiAudio(HdmiAudioDecode::Tv),
                "VSAUDIO TV",
            ),
            (
                VideoSettingValue::AspectRatio(AspectRatio::Normal),
                "VSASPNRM",
            ),
            (VideoSettingValue::VerticalStretch(true), "VSVST ON"),
            (VideoSettingValue::VerticalStretch(false), "VSVST OFF"),
            (
                VideoSettingValue::Resolution(VideoResolution::P1080),
                "VSSC10P",
            ),
            (
                VideoSettingValue::HdmiUpscaler(HdmiUpscaler::Off),
                "VSSCHOFF",
            ),
            (
                VideoSettingValue::HdmiUpscaler(HdmiUpscaler::Resolution(VideoResolution::K8)),
                "VSSCH8K",
            ),
        ];
        for (value, expected) in cases {
            assert_eq!(text(value), expected);
        }
    }
}
