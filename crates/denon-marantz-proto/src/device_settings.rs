//! Settings of the whole receiver that a client can set on telnet.

use core::fmt::Write as _;

use crate::brand::Brand;
use crate::error::Error;
use crate::values::{BluetoothOutputMode, DimmerMode, EcoMode, Illumination, PanelLock};

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
        // Writing to a `String` cannot fail.
        let _ = match self {
            Self::Dimmer(mode) => write!(out, "DIM {}", mode.telnet_code()),
            Self::DimmerNext => write!(out, "DIM SEL"),
            Self::EcoMode(mode) => write!(out, "ECO{}", mode.telnet_code()),
            Self::RemoteLock(on) => write!(out, "SYREMOTE LOCK {}", on_off(on)),
            Self::PanelLock(PanelLock::Off) => write!(out, "SYPANEL LOCK OFF"),
            Self::PanelLock(PanelLock::Panel) => write!(out, "SYPANEL LOCK ON"),
            Self::PanelLock(PanelLock::PanelAndVolume) => write!(out, "SYPANEL+V LOCK ON"),
            Self::BluetoothTransmitter(on) => write!(out, "BTTX {}", on_off(on)),
            Self::BluetoothOutput(mode) => write!(out, "BTTX {}", mode.code()),
            Self::Illumination(mode) => write!(out, "ILB {}", mode.code()),
            Self::IlluminationNext => write!(out, "ILB SEL"),
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
