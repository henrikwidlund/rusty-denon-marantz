//! Typed control of the settings of the whole receiver.

use std::hash::{BuildHasher, Hasher, RandomState};
use std::sync::Mutex;
use std::time::Duration;

use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{
    BluetoothOutputMode, DeviceSettingValue, DigitalCodec, DimmerMode, EcoMode, Illumination,
    PanelLock, Trigger, Zone,
};
use tokio::time::Instant;

use crate::avr::Avr;
use crate::error::Error;

/// Time that a reset key is valid.
const RESET_KEY_LIFETIME: Duration = Duration::from_secs(60);

/// The key that [`DeviceControl::factory_reset`] needs. Get it from
/// [`DeviceControl::reset_key`]. It is valid for one reset and for 60 seconds,
/// and it cannot be copied.
#[derive(Debug, PartialEq, Eq)]
#[must_use]
pub struct ResetKey(u64);

/// The last reset key that was given out.
#[derive(Debug, Default)]
pub(crate) struct ResetKeys(Mutex<Option<(u64, Instant)>>);

impl ResetKeys {
    /// Makes a new key. The key before it stops being valid.
    fn issue(&self) -> ResetKey {
        let key = RandomState::new().build_hasher().finish();
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((key, Instant::now() + RESET_KEY_LIFETIME));
        ResetKey(key)
    }

    /// Accepts the key once. A key that is not the last one, or that is too
    /// old, is refused.
    fn consume(&self, key: &ResetKey) -> Result<(), Error> {
        let mut last = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match *last {
            Some((issued, until)) if issued == key.0 && Instant::now() <= until => {
                *last = None;
                Ok(())
            }
            Some((_, until)) if Instant::now() > until => {
                *last = None;
                Err(Error::ResetKeyInvalid)
            }
            _ => Err(Error::ResetKeyInvalid),
        }
    }
}

/// Controls the settings of the whole receiver. Get it from [`Avr::device`].
///
/// Each method returns when the command is sent, as [`Avr::send`] does.
/// The new value comes as an event and changes the state. Each method returns
/// the errors of [`Avr::send`].
#[derive(Debug, Clone, Copy)]
pub struct DeviceControl<'a> {
    avr: &'a Avr,
}

impl<'a> DeviceControl<'a> {
    pub(crate) const fn new(avr: &'a Avr) -> Self {
        Self { avr }
    }

    /// Sets the brightness of the display.
    ///
    /// # Errors
    ///
    /// See [`DeviceControl`].
    pub async fn set_dimmer(&self, mode: DimmerMode) -> Result<(), Error> {
        self.send(DeviceSettingValue::Dimmer(mode)).await
    }

    /// Goes to the next brightness of the display: bright, dim, dark, off.
    ///
    /// # Errors
    ///
    /// See [`DeviceControl`].
    pub async fn dimmer_next(&self) -> Result<(), Error> {
        self.send(DeviceSettingValue::DimmerNext).await
    }

    /// Sets the eco mode.
    ///
    /// # Errors
    ///
    /// See [`DeviceControl`].
    pub async fn set_eco_mode(&self, mode: EcoMode) -> Result<(), Error> {
        self.send(DeviceSettingValue::EcoMode(mode)).await
    }

    /// Locks (`true`) or unlocks (`false`) the remote control. The receiver
    /// still takes commands on the network.
    ///
    /// # Errors
    ///
    /// See [`DeviceControl`].
    pub async fn set_remote_lock(&self, locked: bool) -> Result<(), Error> {
        self.send(DeviceSettingValue::RemoteLock(locked)).await
    }

    /// Turns video select on (`true`) or off (`false`). When it is on, the
    /// video comes from the video select source, not from the selected source.
    ///
    /// # Errors
    ///
    /// See [`DeviceControl`].
    pub async fn set_video_select(&self, on: bool) -> Result<(), Error> {
        self.send(DeviceSettingValue::VideoSelect(on)).await
    }

    /// Turns video select on and takes the video from a source, by its telnet
    /// name, for example `DVD`. `LAST` takes the source from before.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`] if the name is empty or has a control
    /// character. See also [`DeviceControl`].
    pub async fn select_video_source(&self, name: &str) -> Result<(), Error> {
        self.avr
            .send(Zone::Main, Command::VideoSelectSource(name))
            .await
    }

    /// Selects the source for the recording output (REC SELECT) by its telnet
    /// name, for example `CD`. `SOURCE` ends REC SELECT. Older receivers
    /// only.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`] if the name is empty or has a control
    /// character. See also [`DeviceControl`].
    pub async fn select_record_source(&self, name: &str) -> Result<(), Error> {
        self.avr.send(Zone::Main, Command::RecordSource(name)).await
    }

    /// Locks the panel buttons, with or without the master volume, or unlocks
    /// them. The receiver still takes commands on the network.
    ///
    /// # Errors
    ///
    /// See [`DeviceControl`].
    pub async fn set_panel_lock(&self, lock: PanelLock) -> Result<(), Error> {
        self.send(DeviceSettingValue::PanelLock(lock)).await
    }

    /// Turns the Bluetooth transmitter on (`true`) or off (`false`).
    ///
    /// # Errors
    ///
    /// See [`DeviceControl`].
    pub async fn set_bluetooth_transmitter(&self, on: bool) -> Result<(), Error> {
        self.send(DeviceSettingValue::BluetoothTransmitter(on))
            .await
    }

    /// Sets where the sound goes when the Bluetooth transmitter is on:
    /// Bluetooth and the speakers, or Bluetooth only.
    ///
    /// # Errors
    ///
    /// See [`DeviceControl`].
    pub async fn set_bluetooth_output(&self, mode: BluetoothOutputMode) -> Result<(), Error> {
        self.send(DeviceSettingValue::BluetoothOutput(mode)).await
    }

    /// Sets the brightness of the illumination. Marantz receivers only.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`], and sends nothing, on a Denon
    /// receiver. See also [`DeviceControl`].
    pub async fn set_illumination(&self, mode: Illumination) -> Result<(), Error> {
        self.send(DeviceSettingValue::Illumination(mode)).await
    }

    /// Goes to the next brightness of the illumination: auto, bright, dim,
    /// dark, off. Marantz receivers only.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`], and sends nothing, on a Denon
    /// receiver. See also [`DeviceControl`].
    pub async fn illumination_next(&self) -> Result<(), Error> {
        self.send(DeviceSettingValue::IlluminationNext).await
    }

    /// Sets the digital input mode. The text is from the integration of the
    /// remote. The protocol spec has none.
    ///
    /// # Errors
    ///
    /// See [`DeviceControl`].
    pub async fn set_digital_codec(&self, mode: DigitalCodec) -> Result<(), Error> {
        self.send(DeviceSettingValue::DigitalCodec(mode)).await
    }

    /// Calls favorite 1 to 3. The text is from the integration of the remote.
    /// The protocol spec has none.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`], and sends nothing, for a number that
    /// is not from 1 to 3. See also [`DeviceControl`].
    pub async fn select_favorite(&self, number: u8) -> Result<(), Error> {
        self.send(DeviceSettingValue::Favorite(number)).await
    }

    /// Stores the current settings in favorite 1 to 3. This replaces what the
    /// favorite had.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`], and sends nothing, for a number that
    /// is not from 1 to 3. See also [`DeviceControl`].
    pub async fn store_favorite(&self, number: u8) -> Result<(), Error> {
        self.send(DeviceSettingValue::StoreFavorite(number)).await
    }

    /// Turns a trigger output on (`true`) or off (`false`). The outputs
    /// switch other equipment. A receiver has up to three, and some models
    /// have fewer.
    ///
    /// # Errors
    ///
    /// See [`DeviceControl`].
    pub async fn set_trigger(&self, trigger: Trigger, on: bool) -> Result<(), Error> {
        self.avr
            .send(Zone::Main, Command::SetTrigger(trigger, on))
            .await
    }

    /// Opens or closes the channel level menu on the screen.
    ///
    /// # Errors
    ///
    /// See [`DeviceControl`].
    pub async fn channel_level_menu(&self) -> Result<(), Error> {
        self.send(DeviceSettingValue::ChannelLevelMenu).await
    }

    /// Shows the status of the receiver on the screen, like the status
    /// button. Denon receivers only.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidCommand`], and sends nothing, on a Marantz
    /// receiver. See also [`DeviceControl`].
    pub async fn show_status(&self) -> Result<(), Error> {
        self.send(DeviceSettingValue::Status).await
    }

    /// Restarts the network of the receiver. The telnet connection ends, and
    /// the connection task connects again after a while.
    ///
    /// # Errors
    ///
    /// See [`DeviceControl`].
    pub async fn restart_network(&self) -> Result<(), Error> {
        self.send(DeviceSettingValue::RestartNetwork).await
    }

    /// Gives the key that [`factory_reset`](Self::factory_reset) needs. A
    /// new key makes the one before it invalid.
    pub fn reset_key(&self) -> ResetKey {
        self.avr.reset_keys().issue()
    }

    /// DANGER: resets ALL settings of the receiver to the factory settings:
    /// sources, names, speaker setup, network, everything. This cannot be
    /// undone. The key must be the last one from [`reset_key`](Self::reset_key),
    /// and not older than 60 seconds.
    ///
    /// # Errors
    ///
    /// Returns [`Error::ResetKeyInvalid`], and sends nothing, if the key is
    /// not valid. See also [`DeviceControl`].
    pub async fn factory_reset(&self, key: ResetKey) -> Result<(), Error> {
        self.avr.reset_keys().consume(&key)?;
        self.send(DeviceSettingValue::FactoryReset).await
    }

    async fn send(&self, value: DeviceSettingValue) -> Result<(), Error> {
        self.avr
            .send(Zone::Main, Command::SetDeviceSetting(value))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn a_key_works_once() {
        let keys = ResetKeys::default();
        let key = keys.issue();
        assert!(keys.consume(&key).is_ok());
        assert!(matches!(keys.consume(&key), Err(Error::ResetKeyInvalid)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_key_replaces_the_old_one() {
        let keys = ResetKeys::default();
        let old = keys.issue();
        let new = keys.issue();
        assert_ne!(old, new);
        assert!(matches!(keys.consume(&old), Err(Error::ResetKeyInvalid)));
        assert!(keys.consume(&new).is_ok());
    }

    #[tokio::test(start_paused = true)]
    async fn a_key_expires() {
        let keys = ResetKeys::default();
        let key = keys.issue();
        tokio::time::advance(RESET_KEY_LIFETIME + Duration::from_secs(1)).await;
        assert!(matches!(keys.consume(&key), Err(Error::ResetKeyInvalid)));
    }

    #[test]
    fn a_made_up_key_is_refused() {
        let keys = ResetKeys::default();
        assert!(matches!(
            keys.consume(&ResetKey(1)),
            Err(Error::ResetKeyInvalid)
        ));
        let _real = keys.issue();
        assert!(matches!(
            keys.consume(&ResetKey(1)),
            Err(Error::ResetKeyInvalid)
        ));
    }
}
