//! Settings that only the web control pages give: the menu lock, HDMI
//! control, updates, network control and the streaming services.
//!
//! The page does not send changes. Read it again to read it again.

use crate::error::Error;
use crate::speaker_tables::{number, root};

/// Path of the page.
pub const SYSTEM_SETTINGS_PATH: &str = "/ajax/advanced/get_config?type=3";

/// When the receiver answers on the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NetworkControl {
    /// Always, also in standby.
    Always,
    /// Only while the receiver is on. After it is switched on, the network
    /// needs about a minute before it answers.
    WhenOn,
}

impl NetworkControl {
    /// The value for the number of the page. `2` is not seen on a receiver
    /// yet.
    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            1 => Some(Self::Always),
            2 => Some(Self::WhenOn),
            _ => None,
        }
    }
}

/// Settings that telnet does not report. A value is `None` if the page does
/// not have it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SystemSettings {
    /// When the receiver answers on the network.
    pub network_control: Option<NetworkControl>,
    /// HDMI control (CEC) is on.
    pub hdmi_control: Option<bool>,
    /// Firmware updates are allowed.
    pub allow_update: Option<bool>,
    /// The menu is locked.
    pub menu_lock: Option<bool>,
    /// AirPlay is on.
    pub airplay: Option<bool>,
    /// Spotify Connect is on.
    pub spotify_connect: Option<bool>,
    /// Roon Ready is on.
    pub roon_ready: Option<bool>,
    /// TIDAL Connect is on.
    pub tidal_connect: Option<bool>,
    /// Qobuz Connect is on.
    pub qobuz_connect: Option<bool>,
}

impl SystemSettings {
    /// Reads the page. The page has more, for example the network address,
    /// which is not read.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML or is an HTML page.
    pub fn parse(body: &[u8]) -> Result<Self, Error> {
        let node = root(body)?.node();
        let on = |name: &str| match number::<u8>(node, name) {
            Some(1) => Some(true),
            Some(2) => Some(false),
            _ => None,
        };
        Ok(Self {
            network_control: number::<u8>(node, "NetworkControl")
                .and_then(NetworkControl::from_code),
            hdmi_control: on("HDMIControl"),
            allow_update: on("AllowUpdate"),
            menu_lock: on("MenuLock"),
            airplay: on("AirPlay"),
            spotify_connect: on("Spotify"),
            roon_ready: on("Roon"),
            tidal_connect: on("Tidal"),
            qobuz_connect: on("Qobuz"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A10H: &[u8] = include_bytes!("../tests/data/xml/AVC-A10H-ajax-system-settings.xml");

    #[test]
    fn page_of_the_a10h() {
        let settings = SystemSettings::parse(A10H).unwrap();
        assert_eq!(settings.network_control, Some(NetworkControl::Always));
        assert_eq!(settings.hdmi_control, Some(false));
        assert_eq!(settings.allow_update, Some(true));
        assert_eq!(settings.menu_lock, Some(false));
        assert_eq!(settings.airplay, Some(false));
        assert_eq!(settings.spotify_connect, Some(false));
        assert_eq!(settings.roon_ready, Some(false));
        assert_eq!(settings.tidal_connect, Some(false));
        assert_eq!(settings.qobuz_connect, Some(false));
    }

    #[test]
    fn missing_and_unknown_values_are_none() {
        let settings = SystemSettings::parse(
            b"<AdvancedMenu><NetworkControl>9</NetworkControl><MenuLock>1</MenuLock><AirPlay>x</AirPlay></AdvancedMenu>",
        )
        .unwrap();
        assert_eq!(settings.network_control, None);
        assert_eq!(settings.menu_lock, Some(true));
        assert_eq!(settings.airplay, None);
        assert_eq!(settings.hdmi_control, None);
    }

    #[test]
    fn html_is_an_error() {
        assert!(SystemSettings::parse(b"<html><body>no</body></html>").is_err());
    }
}
