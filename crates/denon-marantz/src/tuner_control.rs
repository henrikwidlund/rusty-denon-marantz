//! Typed control of the tuner.

use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{
    HdRadioBand, HdTuningMode, TUNER_QUERIES, TunerBand, TunerCommand, TunerMode, Zone,
};

use crate::avr::Avr;
use crate::error::Error;

/// Controls the tuner of a receiver. Get it from [`Avr::tuner`].
///
/// Only receivers with a tuner accept the commands, and the source must be
/// the tuner. A receiver sends no error otherwise. The methods with `hd` in the
/// name are for receivers with an HD Radio tuner only.
///
/// Each method returns when the command is sent, as [`Avr::send`] does.
/// Each method returns the errors of [`Avr::send`]. A preset that is not from
/// 1 to 56, or a multicast channel above 8, gives [`Error::InvalidCommand`],
/// and nothing is sent.
#[derive(Debug, Clone, Copy)]
pub struct TunerControl<'a> {
    avr: &'a Avr,
}

impl<'a> TunerControl<'a> {
    pub(crate) const fn new(avr: &'a Avr) -> Self {
        Self { avr }
    }

    /// Tunes to the next frequency.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn frequency_up(&self) -> Result<(), Error> {
        self.send(TunerCommand::FrequencyUp).await
    }

    /// Tunes to the previous frequency.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn frequency_down(&self) -> Result<(), Error> {
        self.send(TunerCommand::FrequencyDown).await
    }

    /// Selects the band. DAB is for DAB models only.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn set_band(&self, band: TunerBand) -> Result<(), Error> {
        self.send(TunerCommand::Band(band)).await
    }

    /// Selects automatic or manual tuning.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn set_mode(&self, mode: TunerMode) -> Result<(), Error> {
        self.send(TunerCommand::Mode(mode)).await
    }

    /// Goes to the next preset.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn preset_up(&self) -> Result<(), Error> {
        self.send(TunerCommand::PresetUp).await
    }

    /// Goes to the previous preset.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn preset_down(&self) -> Result<(), Error> {
        self.send(TunerCommand::PresetDown).await
    }

    /// Goes to a preset, from 1 to 56.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn select_preset(&self, number: u8) -> Result<(), Error> {
        self.send(TunerCommand::Preset(number)).await
    }

    /// Stores the station that is tuned in a preset, from 1 to 56. This
    /// replaces the station that the preset had.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn store_preset(&self, number: u8) -> Result<(), Error> {
        self.send(TunerCommand::StorePreset(number)).await
    }

    /// Tunes the HD Radio tuner to the next frequency.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn hd_frequency_up(&self) -> Result<(), Error> {
        self.send(TunerCommand::HdFrequencyUp).await
    }

    /// Tunes the HD Radio tuner to the previous frequency.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn hd_frequency_down(&self) -> Result<(), Error> {
        self.send(TunerCommand::HdFrequencyDown).await
    }

    /// Selects an HD Radio multicast channel, from 1 to 8, or 0 for the
    /// analog broadcast.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn select_hd_multicast(&self, channel: u8) -> Result<(), Error> {
        self.send(TunerCommand::HdMulticast(channel)).await
    }

    /// Selects the band of the HD Radio tuner.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn set_hd_band(&self, band: HdRadioBand) -> Result<(), Error> {
        self.send(TunerCommand::HdBand(band)).await
    }

    /// Selects the tuning mode of the HD Radio tuner.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn set_hd_mode(&self, mode: HdTuningMode) -> Result<(), Error> {
        self.send(TunerCommand::HdMode(mode)).await
    }

    /// Goes to the next HD Radio preset.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn hd_preset_up(&self) -> Result<(), Error> {
        self.send(TunerCommand::HdPresetUp).await
    }

    /// Goes to the previous HD Radio preset.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn hd_preset_down(&self) -> Result<(), Error> {
        self.send(TunerCommand::HdPresetDown).await
    }

    /// Goes to an HD Radio preset, from 1 to 56.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn select_hd_preset(&self, number: u8) -> Result<(), Error> {
        self.send(TunerCommand::HdPreset(number)).await
    }

    /// Stores the HD Radio station that is tuned in a preset, from 1 to 56.
    /// This replaces the station that the preset had.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn store_hd_preset(&self, number: u8) -> Result<(), Error> {
        self.send(TunerCommand::HdStorePreset(number)).await
    }

    /// Asks the receiver for the frequency, the preset and the tuning mode.
    /// The answers fill the tuner state. The connection also asks once when
    /// the source changes to the tuner. Call this again to follow changes
    /// that the receiver does not report, for example the station name. The
    /// receiver answers for the tuner source only. This always uses telnet.
    /// `Avr::refresh_now_playing` picks telnet or HTTP for you.
    ///
    /// # Errors
    ///
    /// See [`TunerControl`].
    pub async fn request(&self) -> Result<(), Error> {
        for query in TUNER_QUERIES {
            self.avr.send(Zone::Main, Command::Raw(query)).await?;
        }
        Ok(())
    }

    async fn send(&self, command: TunerCommand) -> Result<(), Error> {
        self.avr.send(Zone::Main, Command::Tuner(command)).await
    }
}
