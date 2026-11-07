//! Typed control of one zone on telnet.

use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{
    MuteLevel, PowerOnVolume, RemoteKey, ToneLevel, Volume, VolumeDisplay, VolumeMode, Zone,
    ZoneChannelMode, ZoneHdmiAudio, ZoneSettingValue,
};

use crate::avr::Avr;
use crate::error::Error;

/// Defines a method for each setting that has one value.
macro_rules! setters {
    ($( $name:ident($ty:ty) => $variant:ident, $what:literal; )+) => {
        $(
            #[doc = concat!(
                "Sets ", $what, ".\n\n# Errors\n\n",
                "See [`ZoneControl`]. A zone that does not have the setting gives ",
                "[`Error::InvalidCommand`], and nothing is sent."
            )]
            pub async fn $name(&self, value: $ty) -> Result<(), Error> {
                self.send(Command::SetZoneSetting(ZoneSettingValue::$variant(value))).await
            }
        )+
    };
}

/// Defines a method for each step up or down.
macro_rules! steps {
    ($( $name:ident => $variant:ident, $what:literal; )+) => {
        $(
            #[doc = concat!(
                $what, ". Zone 2 and zone 3 only.\n\n# Errors\n\n",
                "See [`ZoneControl`]. The main zone gives [`Error::InvalidCommand`]."
            )]
            pub async fn $name(&self) -> Result<(), Error> {
                self.send(Command::SetZoneSetting(ZoneSettingValue::$variant)).await
            }
        )+
    };
}

/// Controls one zone of a receiver. Get it from [`Avr::zone`].
///
/// The methods for bass, treble, channel levels, high pass filter, channel
/// mode, HDMI audio and the volume mode are for zone 2 and zone 3. The main
/// zone has [`Avr::tone`] for bass and treble. The volume display is for the
/// main zone. The volume limit, the power on volume and the mute level work
/// for all zones. A method that the zone does not have sends nothing and
/// returns [`Error::InvalidCommand`].
///
/// Each method returns when the command is written, as [`Avr::send`] does.
/// That does not mean that the receiver did it: the new value comes as an
/// event and changes the state. Each method returns the errors of
/// [`Avr::send`].
#[derive(Debug, Clone, Copy)]
pub struct ZoneControl<'a> {
    avr: &'a Avr,
    zone: Zone,
}

impl<'a> ZoneControl<'a> {
    pub(crate) const fn new(avr: &'a Avr, zone: Zone) -> Self {
        Self { avr, zone }
    }

    /// The zone that this controls.
    #[must_use]
    pub const fn zone(&self) -> Zone {
        self.zone
    }

    /// Turns the zone on.
    ///
    /// # Errors
    ///
    /// See [`ZoneControl`].
    pub async fn power_on(&self) -> Result<(), Error> {
        self.send(Command::PowerOn).await
    }

    /// Puts the zone in standby.
    ///
    /// # Errors
    ///
    /// See [`ZoneControl`].
    pub async fn power_standby(&self) -> Result<(), Error> {
        self.send(Command::PowerStandby).await
    }

    /// Mutes (`true`) or unmutes (`false`) the zone.
    ///
    /// # Errors
    ///
    /// See [`ZoneControl`].
    pub async fn set_mute(&self, mute: bool) -> Result<(), Error> {
        self.send(Command::Mute(mute)).await
    }

    /// Sets the volume.
    ///
    /// # Errors
    ///
    /// See [`ZoneControl`].
    pub async fn set_volume(&self, volume: Volume) -> Result<(), Error> {
        self.send(Command::SetVolume(volume)).await
    }

    /// Sets the volume in dB. It is rounded to a step of 0.5 dB.
    ///
    /// # Errors
    ///
    /// Returns [`Error::VolumeOutOfRange`], and sends nothing, if the value is
    /// not from -80 to +18 dB. See also [`ZoneControl`].
    pub async fn set_volume_db(&self, db: f32) -> Result<(), Error> {
        let volume = Volume::from_db_rounded(db).ok_or(Error::VolumeOutOfRange(db))?;
        self.set_volume(volume).await
    }

    /// Raises the volume by one step.
    ///
    /// # Errors
    ///
    /// See [`ZoneControl`].
    pub async fn volume_up(&self) -> Result<(), Error> {
        self.send(Command::VolumeUp).await
    }

    /// Lowers the volume by one step.
    ///
    /// # Errors
    ///
    /// See [`ZoneControl`].
    pub async fn volume_down(&self) -> Result<(), Error> {
        self.send(Command::VolumeDown).await
    }

    /// Selects a source by its telnet name, for example `GAME`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`] if the name is empty or has a control
    /// character. See also [`ZoneControl`].
    pub async fn select_source(&self, name: &str) -> Result<(), Error> {
        self.send(Command::SelectSource(name)).await
    }

    /// Selects a sound mode by name, for example `MOVIE`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::NoSoundMode`], and sends nothing, for a zone other
    /// than the main zone: only the main zone has a sound mode. Returns
    /// [`Error::InvalidCommand`] if the name is empty or has a control
    /// character. See also [`ZoneControl`].
    pub async fn select_sound_mode(&self, name: &str) -> Result<(), Error> {
        if self.zone != Zone::Main {
            return Err(Error::NoSoundMode(self.zone));
        }
        self.send(Command::SelectSoundMode(name)).await
    }

    /// Presses a remote key that has no telnet command, for example
    /// [`RemoteKey::NextTrack`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`] with
    /// [`denon_marantz_proto::Error::UnsupportedKey`] if the brand or the zone
    /// has no such key. See also [`ZoneControl`].
    pub async fn press(&self, key: RemoteKey) -> Result<(), Error> {
        self.send(Command::RemoteKey(key)).await
    }

    /// Presses a Denon key by its number, for example `945`. The zone of this
    /// control gives the category.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`] if the number has more than 4 digits,
    /// or if the receiver is a Marantz. See also [`ZoneControl`].
    pub async fn press_denon_code(&self, code: u16) -> Result<(), Error> {
        self.send(Command::DenonKeyCode(code)).await
    }

    /// Presses a Marantz key by the digits after `RCRC5`, for example
    /// `2708105`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`] if the text is empty or has a
    /// character that is not a digit, or if the receiver is a Denon. See also
    /// [`ZoneControl`].
    pub async fn press_marantz_code(&self, digits: &str) -> Result<(), Error> {
        self.send(Command::MarantzKeyCode(digits)).await
    }

    setters! {
        set_bass(ToneLevel) => Bass, "the bass level, from -10 to +10 dB. Zone 2 and zone 3 only";
        set_treble(ToneLevel) => Treble, "the treble level, from -10 to +10 dB. Zone 2 and zone 3 only";
        set_left_level(i8) => LeftLevel, "the level of the left channel in whole dB, from -12 to 12. Zone 2 and zone 3 only";
        set_right_level(i8) => RightLevel, "the level of the right channel in whole dB, from -12 to 12. Zone 2 and zone 3 only";
        set_high_pass_filter(bool) => HighPassFilter, "the high pass filter on (`true`) or off (`false`). Zone 2 and zone 3 only";
        set_channel_mode(ZoneChannelMode) => ChannelMode, "stereo or mono. Zone 2 and zone 3 only";
        set_hdmi_audio(ZoneHdmiAudio) => HdmiAudio, "where the HDMI audio is decoded. Zone 2 and zone 3 only";
        set_volume_limit(Option<Volume>) => VolumeLimit, "the volume limit in whole dB, or none with `None`. A half step is refused. A limit below the volume lowers the volume";
        set_power_on_volume(PowerOnVolume) => PowerOnVolume, "the volume that the zone has when it is switched on. A level must be a whole dB";
        set_mute_level(MuteLevel) => MuteLevel, "the mute level";
        set_volume_mode(VolumeMode) => VolumeMode, "variable volume, or a fixed volume that must be a whole dB. Zone 2 and zone 3 only";
        set_volume_display(VolumeDisplay) => VolumeDisplay, "how the volume is shown, relative to 0 dB or as an absolute number. Main zone only";
    }

    steps! {
        bass_up => BassUp, "Raises the bass by one step";
        bass_down => BassDown, "Lowers the bass by one step";
        treble_up => TrebleUp, "Raises the treble by one step";
        treble_down => TrebleDown, "Lowers the treble by one step";
        left_level_up => LeftLevelUp, "Raises the level of the left channel by one step";
        left_level_down => LeftLevelDown, "Lowers the level of the left channel by one step";
        right_level_up => RightLevelUp, "Raises the level of the right channel by one step";
        right_level_down => RightLevelDown, "Lowers the level of the right channel by one step";
    }

    async fn send(&self, command: Command<'_>) -> Result<(), Error> {
        self.avr.send(self.zone, command).await
    }
}
