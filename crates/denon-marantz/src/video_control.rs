//! Typed control of the video settings on telnet.

use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{
    AspectRatio, HdmiAudioDecode, HdmiOutput, HdmiUpscaler, VideoProcessingMode, VideoResolution,
    VideoSettingValue, Zone,
};

use crate::avr::Avr;
use crate::error::Error;

/// Controls the video settings of a receiver. Get it from [`Avr::video`].
///
/// A receiver only has the settings that fit its model, for example the
/// aspect ratio and the resolution are for older receivers. It sends no error
/// for a setting that it does not have, so the state keeps the old value.
///
/// Each method returns when the command is written, as [`Avr::send`] does.
/// The new value comes as an event and changes the state. Each method returns
/// the errors of [`Avr::send`].
#[derive(Debug, Clone, Copy)]
pub struct VideoControl<'a> {
    avr: &'a Avr,
}

/// Defines a method for each setting.
macro_rules! setters {
    ($( $name:ident($ty:ty) => $variant:ident, $what:literal; )+) => {
        $(
            #[doc = concat!("Sets ", $what, ".\n\n# Errors\n\nSee [`VideoControl`].")]
            pub async fn $name(&self, value: $ty) -> Result<(), Error> {
                self.send(VideoSettingValue::$variant(value)).await
            }
        )+
    };
}

impl<'a> VideoControl<'a> {
    pub(crate) const fn new(avr: &'a Avr) -> Self {
        Self { avr }
    }

    setters! {
        set_processing_mode(VideoProcessingMode) => ProcessingMode, "the video processing mode";
        set_hdmi_output(HdmiOutput) => HdmiOutput, "the HDMI monitor output";
        set_hdmi_audio(HdmiAudioDecode) => HdmiAudio, "where the HDMI audio is decoded";
        set_aspect_ratio(AspectRatio) => AspectRatio, "the aspect ratio. Older receivers only";
        set_vertical_stretch(bool) => VerticalStretch, "the vertical stretch on (`true`) or off (`false`). Older receivers only";
        set_resolution(VideoResolution) => Resolution, "the resolution of the analog to HDMI conversion. Older receivers only";
        set_hdmi_upscaler(HdmiUpscaler) => HdmiUpscaler, "the HDMI upscaler";
    }

    async fn send(&self, value: VideoSettingValue) -> Result<(), Error> {
        self.avr
            .send(Zone::Main, Command::SetVideoSetting(value))
            .await
    }
}
