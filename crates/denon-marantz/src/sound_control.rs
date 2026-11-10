//! Typed control of the sound settings on telnet.

use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{
    AudioRestorer, Auro3dMode, AuroMatic3dPreset, CenterWidthName, Channel, ChannelLevel,
    ChannelVolumeCommand, DacFilter, DialogEnhancerLevel, Drc, EffectSpeakerSelection,
    FrontSpeakers, ImaxAudio, ImaxMode, ImaxSubwooferOutput, InputMode, Mdax, RoomSize,
    SoundSettingValue, SoundStep, SpeakerPreset, SurroundLevelCompensation, Zone,
};

use crate::avr::Avr;
use crate::error::Error;

/// Controls the sound settings of a receiver. Get it from [`Avr::sound`].
///
/// A receiver can ignore a setting that does not apply, for example one that
/// a sound mode or a Dirac Live filter turns off. It sends no error then, so
/// the state keeps the old value.
///
/// Each method returns when the command is written, as [`Avr::telnet_send`] does.
/// The new value comes as an event and changes the state. Each method returns
/// the errors of [`Avr::telnet_send`]. A number that the receiver does not accept
/// gives [`Error::InvalidCommand`], and nothing is sent.
#[derive(Debug, Clone, Copy)]
pub struct SoundControl<'a> {
    avr: &'a Avr,
}

/// Defines the methods that raise and lower a setting by one step.
macro_rules! steps {
    ($( $up:ident, $down:ident => $step:ident, $what:literal; )+) => {
        $(
            #[doc = concat!("Raises ", $what, " by one step.\n\n# Errors\n\nSee [`SoundControl`].")]
            pub async fn $up(&self) -> Result<(), Error> {
                self.send(SoundSettingValue::StepUp(SoundStep::$step)).await
            }

            #[doc = concat!("Lowers ", $what, " by one step.\n\n# Errors\n\nSee [`SoundControl`].")]
            pub async fn $down(&self) -> Result<(), Error> {
                self.send(SoundSettingValue::StepDown(SoundStep::$step)).await
            }
        )+
    };
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
        set_loudness_management(bool) => LoudnessManagement, "loudness management on (`true`) or off (`false`)";
        set_speaker_virtualizer(bool) => SpeakerVirtualizer, "the speaker virtualizer on (`true`) or off (`false`)";
        set_front_speakers(FrontSpeakers) => FrontSpeakers, "the front speaker output. The receiver needs \"Amp assign = Front B\" in its setup";
        set_speaker_preset(SpeakerPreset) => SpeakerPreset, "the speaker preset";
        set_effect(bool) => Effect, "the effect on (`true`) or off (`false`). Older receivers only";
        set_effect_level(u8) => EffectLevel, "the effect level, from 1 to 15";
        set_center_image(u8) => CenterImage, "the center image in tenths, from 0 to 10 (0.0 to 1.0). Older receivers only";
        set_center_gain(u8) => CenterGain, "the center gain in tenths, from 0 to 10 (0.0 to 1.0). Older Marantz receivers only";
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

    /// Sets the center width, from 0 to 7. Receivers use the name `PSCEN` or
    /// `PSCEW`. The method uses the name that the receiver reported. Before
    /// the receiver has reported one, it sends both names and the receiver
    /// drops the one it does not know.
    ///
    /// # Errors
    ///
    /// See [`SoundControl`].
    pub async fn set_center_width(&self, value: u8) -> Result<(), Error> {
        self.center_width(
            SoundSettingValue::CenterWidth(value),
            SoundSettingValue::CenterWidthCew(value),
        )
        .await
    }

    /// Raises the center width by one step. The names are as in
    /// [`SoundControl::set_center_width`].
    ///
    /// # Errors
    ///
    /// See [`SoundControl`].
    pub async fn center_width_up(&self) -> Result<(), Error> {
        self.center_width(
            SoundSettingValue::StepUp(SoundStep::CenterWidth),
            SoundSettingValue::StepUp(SoundStep::CenterWidthCew),
        )
        .await
    }

    /// Lowers the center width by one step. The names are as in
    /// [`SoundControl::set_center_width`].
    ///
    /// # Errors
    ///
    /// See [`SoundControl`].
    pub async fn center_width_down(&self) -> Result<(), Error> {
        self.center_width(
            SoundSettingValue::StepDown(SoundStep::CenterWidth),
            SoundSettingValue::StepDown(SoundStep::CenterWidthCew),
        )
        .await
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

    steps! {
        audio_delay_up, audio_delay_down => AudioDelay, "the audio delay";
        delay_time_up, delay_time_down => DelayTime, "the delay time (Denon receivers)";
        dialog_control_up, dialog_control_down => DialogControl, "the dialog control";
        dimension_up, dimension_down => Dimension, "the dimension";
        lfe_level_up, lfe_level_down => LfeLevel, "the LFE level";
        bass_sync_up, bass_sync_down => BassSync, "the bass sync";
        auro_matic_strength_up, auro_matic_strength_down => AuroMaticStrength, "the Auro-Matic strength";
        center_level_up, center_level_down => CenterLevel, "the level of the center speaker";
        effect_level_up, effect_level_down => EffectLevel, "the effect level";
        center_image_up, center_image_down => CenterImage, "the center image (older receivers)";
        center_gain_up, center_gain_down => CenterGain, "the center gain (older Marantz receivers)";
    }

    /// Raises the level of subwoofer 1 to 4 by one step.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`], and sends nothing, for a number
    /// that is not from 1 to 4. See also [`SoundControl`].
    pub async fn subwoofer_level_up(&self, number: u8) -> Result<(), Error> {
        self.send(SoundSettingValue::StepUp(SoundStep::SubwooferLevel(number)))
            .await
    }

    /// Lowers the level of subwoofer 1 to 4 by one step.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`], and sends nothing, for a number
    /// that is not from 1 to 4. See also [`SoundControl`].
    pub async fn subwoofer_level_down(&self, number: u8) -> Result<(), Error> {
        self.send(SoundSettingValue::StepDown(SoundStep::SubwooferLevel(
            number,
        )))
        .await
    }

    /// Sets the level of a speaker channel of the main zone. The receiver does
    /// not accept a change while a Dirac ART filter is active. Other Dirac
    /// filters do not stop it.
    ///
    /// # Errors
    ///
    /// See [`SoundControl`].
    pub async fn set_channel_volume(
        &self,
        channel: Channel,
        level: ChannelLevel,
    ) -> Result<(), Error> {
        self.channel_volume(ChannelVolumeCommand::Set(channel, level))
            .await
    }

    /// Raises the level of a speaker channel of the main zone by 0.5 dB. Dirac
    /// Live must be off for the receiver to accept a change.
    ///
    /// # Errors
    ///
    /// See [`SoundControl`].
    pub async fn channel_volume_up(&self, channel: Channel) -> Result<(), Error> {
        self.channel_volume(ChannelVolumeCommand::Up(channel)).await
    }

    /// Lowers the level of a speaker channel of the main zone by 0.5 dB. Dirac
    /// Live must be off for the receiver to accept a change.
    ///
    /// # Errors
    ///
    /// See [`SoundControl`].
    pub async fn channel_volume_down(&self, channel: Channel) -> Result<(), Error> {
        self.channel_volume(ChannelVolumeCommand::Down(channel))
            .await
    }

    /// Sets the level of all speaker channels of the main zone to 0 dB. This
    /// overwrites all levels at once and cannot be undone by the library.
    /// The receiver does not accept it while a Dirac ART filter is active. Not
    /// tested on a receiver.
    ///
    /// # Errors
    ///
    /// See [`SoundControl`].
    pub async fn reset_channel_volumes(&self) -> Result<(), Error> {
        self.channel_volume(ChannelVolumeCommand::ResetAll).await
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
    /// analog input. The receiver reports the mode as an event, and the state
    /// keeps it in `input_mode`.
    ///
    /// # Errors
    ///
    /// See [`SoundControl`].
    pub async fn set_input_mode(&self, mode: InputMode) -> Result<(), Error> {
        self.avr
            .telnet_send(Zone::Main, Command::SetInputMode(mode))
            .await
    }

    /// Sends the command with the name that the receiver reported, or both.
    async fn center_width(
        &self,
        cen: SoundSettingValue,
        cew: SoundSettingValue,
    ) -> Result<(), Error> {
        let name = self
            .avr
            .state()
            .borrow()
            .device()
            .sound_settings
            .center_width_name;
        match name {
            Some(CenterWidthName::Cen) => self.send(cen).await,
            Some(CenterWidthName::Cew) => self.send(cew).await,
            None => {
                self.send(cen).await?;
                self.send(cew).await
            }
        }
    }

    async fn channel_volume(&self, command: ChannelVolumeCommand) -> Result<(), Error> {
        self.avr
            .telnet_send(Zone::Main, Command::SetChannelVolume(command))
            .await
    }

    async fn send(&self, value: SoundSettingValue) -> Result<(), Error> {
        self.avr
            .telnet_send(Zone::Main, Command::SetSoundSetting(value))
            .await
    }
}
