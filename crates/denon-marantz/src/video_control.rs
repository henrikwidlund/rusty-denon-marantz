//! Typed control of the video settings on telnet.

use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{
    AspectRatio, HdmiAudioDecode, HdmiOutput, HdmiUpscaler, NoiseReduction, PictureMode,
    PictureSettingValue, VideoProcessingMode, VideoResolution, VideoSettingValue, Zone,
};

use crate::avr::Avr;
use crate::error::Error;

/// Controls the video settings of a receiver. Get it from [`Avr::video`].
///
/// A receiver only has the settings that fit its model, for example the
/// aspect ratio and the resolution are for older receivers. It sends no error
/// for a setting that it does not have, so the state keeps the old value.
///
/// Each method returns when the command is written, as [`Avr::telnet_send`] does.
/// The new value comes as an event and changes the state. Each method returns
/// the errors of [`Avr::telnet_send`].
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

/// Defines a method for each picture setting that has one value.
macro_rules! picture_setters {
    ($( $name:ident($ty:ty) => $variant:ident, $what:literal; )+) => {
        $(
            #[doc = concat!(
                "Sets ", $what, ". Older receivers only.\n\n# Errors\n\nSee [`VideoControl`]. ",
                "A value outside the range gives [`Error::InvalidCommand`], and nothing is sent."
            )]
            pub async fn $name(&self, value: $ty) -> Result<(), Error> {
                self.send_picture(PictureSettingValue::$variant(value)).await
            }
        )+
    };
}

/// Defines a method for each picture setting step.
macro_rules! picture_steps {
    ($( $name:ident => $variant:ident, $what:literal; )+) => {
        $(
            #[doc = concat!($what, ". Older receivers only.\n\n# Errors\n\nSee [`VideoControl`].")]
            pub async fn $name(&self) -> Result<(), Error> {
                self.send_picture(PictureSettingValue::$variant).await
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

    picture_setters! {
        set_picture_mode(PictureMode) => Mode, "the picture mode";
        set_contrast(i8) => Contrast, "the contrast, from -50 to +50";
        set_brightness(i8) => Brightness, "the brightness, from -50 to +50";
        set_saturation(i8) => Saturation, "the saturation, from -50 to +50";
        set_noise_reduction(NoiseReduction) => NoiseReduction, "the noise reduction";
        set_enhancer(u8) => Enhancer, "the enhancer, from 0 to 12";
    }

    picture_steps! {
        contrast_up => ContrastUp, "Raises the contrast by one step";
        contrast_down => ContrastDown, "Lowers the contrast by one step";
        brightness_up => BrightnessUp, "Raises the brightness by one step";
        brightness_down => BrightnessDown, "Lowers the brightness by one step";
        saturation_up => SaturationUp, "Raises the saturation by one step";
        saturation_down => SaturationDown, "Lowers the saturation by one step";
        enhancer_up => EnhancerUp, "Raises the enhancer by one step";
        enhancer_down => EnhancerDown, "Lowers the enhancer by one step";
    }

    async fn send_picture(&self, value: PictureSettingValue) -> Result<(), Error> {
        self.avr
            .telnet_send(Zone::Main, Command::SetPictureSetting(value))
            .await
    }

    async fn send(&self, value: VideoSettingValue) -> Result<(), Error> {
        self.avr
            .telnet_send(Zone::Main, Command::SetVideoSetting(value))
            .await
    }
}
