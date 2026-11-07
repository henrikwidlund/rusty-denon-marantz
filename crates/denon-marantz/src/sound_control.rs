//! Typed control of the sound settings on telnet.

use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{
    AudioRestorer, Auro3dMode, AuroMatic3dPreset, ChannelLevel, DacFilter, DialogEnhancerLevel,
    Drc, EffectSpeakerSelection, FrontSpeakers, ImaxAudio, ImaxMode, ImaxSubwooferOutput,
    InputMode, Mdax, RoomSize, SoundSettingValue, SpeakerPreset, SurroundLevelCompensation, Zone,
};

use crate::avr::Avr;
use crate::error::Error;

/// Controls the sound settings of a receiver. Get it from [`Avr::sound`].
///
/// A receiver can ignore a setting that does not apply, for example one that
/// a sound mode or a Dirac Live filter turns off. It sends no error then, so
/// the state keeps the old value.
///
/// Each method returns when the command is written, as [`Avr::send`] does.
/// The new value comes as an event and changes the state. Each method returns
/// the errors of [`Avr::send`]. A number that the receiver does not accept
/// gives [`Error::InvalidCommand`], and nothing is sent.
#[derive(Debug, Clone, Copy)]
pub struct SoundControl<'a> {
    avr: &'a Avr,
}

/// Defines a method for each setting that has one value.
macro_rules! setters {
    ($( $name:ident($ty:ty) => $variant:ident, $what:literal; )+) => {
        $(
            #[doc = concat!("Sets ", $what, ".\n\n# Errors\n\nSee [`SoundControl`].")]
            pub async fn $name(&self, value: $ty) -> Result<(), Error> {
                self.send(SoundSettingValue::$variant(value)).await
            }
        )+
    };
}

impl<'a> SoundControl<'a> {
    pub(crate) const fn new(avr: &'a Avr) -> Self {
        Self { avr }
    }

    setters! {
        set_audio_restorer(AudioRestorer) => AudioRestorer, "the audio restorer";
        set_room_size(RoomSize) => RoomSize, "the room size";
        set_surround_level(SurroundLevelCompensation) => SurroundLevel, "the surround level compensation";
        set_dialog_enhancer(DialogEnhancerLevel) => DialogEnhancer, "the dialog enhancer";
        set_drc(Drc) => Drc, "the dynamic range compression";
        set_auro_3d_mode(Auro3dMode) => Auro3dMode, "the Auro-3D mode";
        set_auro_matic_preset(AuroMatic3dPreset) => AuroMaticPreset, "the Auro-Matic preset";
        set_auro_matic_strength(u8) => AuroMaticStrength, "the Auro-Matic strength, from 0 to 15";
        set_effect_speakers(EffectSpeakerSelection) => EffectSpeakers, "the speakers for the effect channels";
        set_mdax(Mdax) => Mdax, "MDAX. Marantz receivers only";
        set_dac_filter(DacFilter) => DacFilter, "the DAC filter. Marantz receivers only";
        set_imax(ImaxMode) => Imax, "the IMAX mode";
        set_imax_audio(ImaxAudio) => ImaxAudio, "the IMAX audio settings";
        set_imax_subwoofer_output(ImaxSubwooferOutput) => ImaxSubwooferOutput, "the IMAX subwoofer output";
        set_imax_subwoofer_mode(bool) => ImaxSubwooferMode, "the IMAX subwoofer mode on (`true`) or off (`false`)";
        set_imax_high_pass(u16) => ImaxHighPass, "the IMAX high pass filter in Hz: 40, 60, 80, 90, 100, 110, 120, 150, 180, 200 or 250";
        set_imax_low_pass(u16) => ImaxLowPass, "the IMAX low pass filter in Hz: 80, 90, 100, 110, 120, 150, 180, 200 or 250";
        set_subwoofer(bool) => Subwoofer, "the subwoofer on (`true`) or off (`false`)";
        set_lfe_level(u8) => LfeLevel, "the LFE level as dB below 0 dB, from 0 to 10";
        set_center_level(ChannelLevel) => CenterLevel, "the level of the center speaker";
        set_center_spread(bool) => CenterSpread, "center spread on (`true`) or off (`false`)";
        set_center_width(u8) => CenterWidth, "the center width, from 0 to 7";
        set_loudness_management(bool) => LoudnessManagement, "loudness management on (`true`) or off (`false`)";
        set_speaker_virtualizer(bool) => SpeakerVirtualizer, "the speaker virtualizer on (`true`) or off (`false`)";
        set_front_speakers(FrontSpeakers) => FrontSpeakers, "the front speaker output. The receiver needs \"Amp assign = Front B\" in its setup";
        set_speaker_preset(SpeakerPreset) => SpeakerPreset, "the speaker preset";
        set_tactile_transducer(bool) => TactileTransducer, "the tactile transducer on (`true`) or off (`false`)";
        set_tactile_low_pass(u16) => TactileLowPass, "the low pass filter of the tactile transducer in Hz: 40, 60, 80, 90, 100, 110, 120, 150, 180, 200 or 250";
        set_graphic_eq(bool) => GraphicEq, "the graphic EQ on (`true`) or off (`false`)";
        set_headphone_eq(bool) => HeadphoneEq, "the headphone EQ on (`true`) or off (`false`)";
        set_cinema_eq(bool) => CinemaEq, "the cinema EQ on (`true`) or off (`false`)";
        set_neural_x(bool) => NeuralX, "Neural:X on (`true`) or off (`false`)";
        set_panorama(bool) => Panorama, "panorama on (`true`) or off (`false`)";
        set_dimension(u8) => Dimension, "the dimension, from 0 to 6";
        set_bass_sync(u8) => BassSync, "the bass sync, from 0 to 16";
        set_dialog_control(u8) => DialogControl, "the dialog control in dB, from 0 to 6";
        set_delay_time(u16) => DelayTime, "the delay time in ms, from 0 to 300";
        set_audio_delay(u16) => AudioDelay, "the audio delay in ms, from 0 to 500";
    }

    /// Sets the level of subwoofer 1 to 4.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`], and sends nothing, if `number` is
    /// not from 1 to 4. See also [`SoundControl`].
    pub async fn set_subwoofer_level(&self, number: u8, level: ChannelLevel) -> Result<(), Error> {
        self.send(SoundSettingValue::SubwooferLevel { number, level })
            .await
    }

    /// Raises the level of the tactile transducer by one step.
    ///
    /// # Errors
    ///
    /// See [`SoundControl`].
    pub async fn tactile_level_up(&self) -> Result<(), Error> {
        self.send(SoundSettingValue::TactileLevelUp).await
    }

    /// Lowers the level of the tactile transducer by one step.
    ///
    /// # Errors
    ///
    /// See [`SoundControl`].
    pub async fn tactile_level_down(&self) -> Result<(), Error> {
        self.send(SoundSettingValue::TactileLevelDown).await
    }

    /// Sets the audio input mode: automatic, or a forced HDMI, digital or
    /// analog input. The receiver does not report the mode back, so the state
    /// does not have it.
    ///
    /// # Errors
    ///
    /// See [`SoundControl`].
    pub async fn set_input_mode(&self, mode: InputMode) -> Result<(), Error> {
        self.avr.send(Zone::Main, Command::SetInputMode(mode)).await
    }

    async fn send(&self, value: SoundSettingValue) -> Result<(), Error> {
        self.avr
            .send(Zone::Main, Command::SetSoundSetting(value))
            .await
    }
}
