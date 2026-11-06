//! Typed control of one zone on telnet.

use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{Volume, Zone};

use crate::avr::Avr;
use crate::error::Error;

/// Controls one zone of a receiver. Get it from [`Avr::zone`].
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
    /// Returns [`Error::InvalidRequest`], and sends nothing, if the value is
    /// not from -80 to +18 dB. See also [`ZoneControl`].
    pub async fn set_volume_db(&self, db: f32) -> Result<(), Error> {
        let volume = Volume::from_db_rounded(db).ok_or_else(|| {
            Error::InvalidRequest(format!("volume {db} dB is not from -80 to 18 dB"))
        })?;
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
    /// Returns [`Error::InvalidRequest`], and sends nothing, for a zone other
    /// than the main zone: only the main zone has a sound mode. Returns
    /// [`Error::InvalidCommand`] if the name is empty or has a control
    /// character. See also [`ZoneControl`].
    pub async fn select_sound_mode(&self, name: &str) -> Result<(), Error> {
        if self.zone != Zone::Main {
            return Err(Error::InvalidRequest(format!(
                "{} has no sound mode",
                self.zone.name()
            )));
        }
        self.send(Command::SelectSoundMode(name)).await
    }

    async fn send(&self, command: Command<'_>) -> Result<(), Error> {
        self.avr.send(self.zone, command).await
    }
}
