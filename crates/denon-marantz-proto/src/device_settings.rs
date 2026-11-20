//! Settings of the whole receiver that a client can set on telnet.

use core::fmt::Write as _;

use crate::brand::Brand;
use crate::error::Error;
use crate::values::{
    BluetoothOutputMode, DigitalCodec, DimmerMode, EcoMode, Illumination, PanelLock,
};

/// Bluetooth output levels: 30 is -50 dB, 80 is 0 dB and 90 is +10 dB.
pub(crate) const BLUETOOTH_LEVELS: core::ops::RangeInclusive<u8> = 30..=90;

/// A setting of the whole receiver, with the value to set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DeviceSettingValue {
    /// Sets the brightness of the display.
    Dimmer(DimmerMode),
    /// Goes to the next brightness of the display: bright, dim, dark, off.
    DimmerNext,
    /// Sets the eco mode.
    EcoMode(EcoMode),
    /// Locks (`true`) or unlocks (`false`) the remote control.
    RemoteLock(bool),
    /// Turns video select on (`true`) or off (`false`). When it is on, the
    /// video comes from the video select source.
    VideoSelect(bool),
    /// Locks the panel buttons, with or without the master volume, or
    /// unlocks them.
    PanelLock(PanelLock),
    /// Turns the Bluetooth transmitter on (`true`) or off (`false`).
    BluetoothTransmitter(bool),
    /// Sets where the sound goes when the Bluetooth transmitter is on.
    BluetoothOutput(BluetoothOutputMode),
    /// Sets the brightness of the illumination. Marantz receivers only.
    Illumination(Illumination),
    /// Goes to the next brightness of the illumination: auto, bright, dim,
    /// dark, off. Marantz receivers only.
    IlluminationNext,
    /// Bluetooth output level, from 30 (-50 dB) to 90 (+10 dB). 80 is 0 dB.
    BluetoothLevel(u8),
    /// Raises the Bluetooth output level by one step.
    BluetoothLevelUp,
    /// Lowers the Bluetooth output level by one step.
    BluetoothLevelDown,
    /// Sets the digital input mode. The text is from the integration of the
    /// remote, the protocol spec has none.
    DigitalCodec(DigitalCodec),
    /// Calls favorite 1 to 4. Favorite 4 exists only on some older Marantz
    /// receivers.
    Favorite(u8),
    /// Stores the current settings in favorite 1 to 4.
    StoreFavorite(u8),
    /// Opens or closes the channel level menu on the screen.
    ChannelLevelMenu,
    /// Shows the status of the receiver on the screen, like the status button.
    /// Denon receivers only.
    Status,
    /// Restarts the network of the receiver. The connection ends.
    RestartNetwork,
    /// DANGER: resets ALL settings of the receiver. The facade asks for a
    /// reset key before it sends this.
    FactoryReset,
}

impl DeviceSettingValue {
    /// Appends the command text, without the line end.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnsupportedKey`] for the status on a Marantz receiver,
    /// and for the illumination on a Denon receiver.
    pub(crate) fn write(self, brand: Brand, out: &mut String) -> Result<(), Error> {
        let wrong_brand = match self {
            Self::Status => brand != Brand::Denon,
            Self::Illumination(_) | Self::IlluminationNext => brand != Brand::Marantz,
            _ => false,
        };
        if wrong_brand {
            return Err(Error::UnsupportedKey);
        }
        if let Self::BluetoothLevel(level) = self
            && !BLUETOOTH_LEVELS.contains(&level)
        {
            return Err(Error::ValueOutOfRange {
                what: "Bluetooth level",
            });
        }
        if let Self::Favorite(number) | Self::StoreFavorite(number) = self
            && !(1..=4).contains(&number)
        {
            return Err(Error::ValueOutOfRange { what: "favorite" });
        }
        // Writing to a `String` cannot fail.
        let _ = match self {
            Self::Dimmer(mode) => write!(out, "DIM {}", mode.telnet_code()),
            Self::DimmerNext => write!(out, "DIM SEL"),
            Self::EcoMode(mode) => write!(out, "ECO{}", mode.telnet_code()),
            Self::RemoteLock(on) => write!(out, "SYREMOTE LOCK {}", on_off(on)),
            Self::VideoSelect(on) => write!(out, "SV{}", on_off(on)),
            Self::PanelLock(PanelLock::Off) => write!(out, "SYPANEL LOCK OFF"),
            Self::PanelLock(PanelLock::Panel) => write!(out, "SYPANEL LOCK ON"),
            Self::PanelLock(PanelLock::PanelAndVolume) => write!(out, "SYPANEL+V LOCK ON"),
            Self::BluetoothTransmitter(on) => write!(out, "BTTX {}", on_off(on)),
            Self::BluetoothOutput(mode) => write!(out, "BTTX {}", mode.code()),
            Self::BluetoothLevel(level) => write!(out, "BTLEV {level:02}"),
            Self::BluetoothLevelUp => write!(out, "BTLEV UP"),
            Self::BluetoothLevelDown => write!(out, "BTLEV DOWN"),
            Self::Illumination(mode) => write!(out, "ILB {}", mode.code()),
            Self::IlluminationNext => write!(out, "ILB SEL"),
            Self::DigitalCodec(mode) => write!(out, "DC{}", mode.code()),
            Self::Favorite(number) => write!(out, "ZMFAVORITE{number}"),
            Self::StoreFavorite(number) => write!(out, "ZMFAVORITE{number} MEMORY"),
            Self::ChannelLevelMenu => write!(out, "MNCHL"),
            Self::Status => write!(out, "RCSHP0230030"),
            Self::RestartNetwork => write!(out, "NSRBT"),
            Self::FactoryReset => write!(out, "SYRST"),
        };
        Ok(())
    }
}

const fn on_off(on: bool) -> &'static str {
    if on { "ON" } else { "OFF" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn texts() {
        let cases = [
            (DeviceSettingValue::Dimmer(DimmerMode::Bright), "DIM BRI"),
            (DeviceSettingValue::Dimmer(DimmerMode::Dim), "DIM DIM"),
            (DeviceSettingValue::Dimmer(DimmerMode::Dark), "DIM DAR"),
            (DeviceSettingValue::Dimmer(DimmerMode::Off), "DIM OFF"),
            (DeviceSettingValue::DimmerNext, "DIM SEL"),
            (DeviceSettingValue::EcoMode(EcoMode::On), "ECOON"),
            (DeviceSettingValue::EcoMode(EcoMode::Auto), "ECOAUTO"),
            (DeviceSettingValue::EcoMode(EcoMode::Off), "ECOOFF"),
            (DeviceSettingValue::RemoteLock(true), "SYREMOTE LOCK ON"),
            (DeviceSettingValue::RemoteLock(false), "SYREMOTE LOCK OFF"),
            (DeviceSettingValue::VideoSelect(true), "SVON"),
            (DeviceSettingValue::VideoSelect(false), "SVOFF"),
            (
                DeviceSettingValue::PanelLock(PanelLock::Off),
                "SYPANEL LOCK OFF",
            ),
            (
                DeviceSettingValue::PanelLock(PanelLock::Panel),
                "SYPANEL LOCK ON",
            ),
            (
                DeviceSettingValue::PanelLock(PanelLock::PanelAndVolume),
                "SYPANEL+V LOCK ON",
            ),
            (DeviceSettingValue::BluetoothTransmitter(true), "BTTX ON"),
            (DeviceSettingValue::BluetoothTransmitter(false), "BTTX OFF"),
            (
                DeviceSettingValue::BluetoothOutput(BluetoothOutputMode::BluetoothAndSpeakers),
                "BTTX SP",
            ),
            (
                DeviceSettingValue::BluetoothOutput(BluetoothOutputMode::BluetoothOnly),
                "BTTX BT",
            ),
            (DeviceSettingValue::DigitalCodec(DigitalCodec::Pcm), "DCPCM"),
            (
                DeviceSettingValue::DigitalCodec(DigitalCodec::Auto),
                "DCAUTO",
            ),
            (DeviceSettingValue::Favorite(2), "ZMFAVORITE2"),
            (DeviceSettingValue::StoreFavorite(3), "ZMFAVORITE3 MEMORY"),
            (DeviceSettingValue::Favorite(4), "ZMFAVORITE4"),
            (DeviceSettingValue::BluetoothLevel(30), "BTLEV 30"),
            (DeviceSettingValue::BluetoothLevel(90), "BTLEV 90"),
            (DeviceSettingValue::BluetoothLevelUp, "BTLEV UP"),
            (DeviceSettingValue::BluetoothLevelDown, "BTLEV DOWN"),
            (DeviceSettingValue::ChannelLevelMenu, "MNCHL"),
            (DeviceSettingValue::Status, "RCSHP0230030"),
            (DeviceSettingValue::RestartNetwork, "NSRBT"),
            (DeviceSettingValue::FactoryReset, "SYRST"),
        ];
        for (value, expected) in cases {
            let mut out = String::new();
            value.write(Brand::Denon, &mut out).unwrap();
            assert_eq!(out, expected, "{value:?}");
        }
    }

    #[test]
    fn a_bluetooth_level_is_from_30_to_90() {
        for level in [0, 29, 91] {
            let mut out = String::new();
            assert_eq!(
                DeviceSettingValue::BluetoothLevel(level).write(Brand::Denon, &mut out),
                Err(Error::ValueOutOfRange {
                    what: "Bluetooth level"
                })
            );
        }
    }

    #[test]
    fn a_favorite_is_from_1_to_4() {
        for number in [0, 5] {
            for value in [
                DeviceSettingValue::Favorite(number),
                DeviceSettingValue::StoreFavorite(number),
            ] {
                let mut out = String::new();
                assert_eq!(
                    value.write(Brand::Denon, &mut out),
                    Err(Error::ValueOutOfRange { what: "favorite" })
                );
                assert_eq!(out, "");
            }
        }
    }

    #[test]
    fn the_illumination_is_for_marantz_only() {
        let mut out = String::new();
        for value in [
            DeviceSettingValue::Illumination(Illumination::Auto),
            DeviceSettingValue::IlluminationNext,
        ] {
            assert_eq!(
                value.write(Brand::Denon, &mut out),
                Err(Error::UnsupportedKey)
            );
        }
        assert_eq!(out, "");
        DeviceSettingValue::Illumination(Illumination::Bright)
            .write(Brand::Marantz, &mut out)
            .unwrap();
        assert_eq!(out, "ILB BRI");
        out.clear();
        DeviceSettingValue::IlluminationNext
            .write(Brand::Marantz, &mut out)
            .unwrap();
        assert_eq!(out, "ILB SEL");
    }

    #[test]
    fn the_status_is_for_denon_only() {
        let mut out = String::new();
        assert_eq!(
            DeviceSettingValue::Status.write(Brand::Marantz, &mut out),
            Err(Error::UnsupportedKey)
        );
        assert_eq!(out, "");
        DeviceSettingValue::RestartNetwork
            .write(Brand::Marantz, &mut out)
            .unwrap();
        assert_eq!(out, "NSRBT");
    }
}
