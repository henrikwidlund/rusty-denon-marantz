//! Input source names.
//!
//! Receivers report the display names of inputs in XML and the short names on
//! telnet. These functions convert between them.

use crate::now_playing::PlayingKind;

/// Telnet source name for an input name from XML.
///
/// Returns the input unchanged if there is no special mapping.
#[must_use]
pub fn xml_to_telnet_source(name: &str) -> &str {
    match name {
        "TV AUDIO" => "TV",
        "iPod/USB" => "USB/IPOD",
        "Bluetooth" => "BT",
        "Blu-ray" => "BD",
        "CBL/SAT" => "SAT/CBL",
        "NETWORK" => "NET",
        "Media Player" => "MPLAY",
        "AUX" => "AUX1",
        "Tuner" | "FM" => "TUNER",
        "SpotifyConnect" => "Spotify Connect",
        other => other,
    }
}

/// Telnet source names and their display names.
const DISPLAY_NAMES: [(&str, &str); 5] = [
    ("FAVORITES", "Favorites"),
    ("FLICKR", "Flickr"),
    ("IRADIO", "Internet Radio"),
    ("SERVER", "Media Server"),
    ("SPOTIFY", "Spotify"),
];

/// Display name for a telnet source name.
///
/// Returns the input unchanged if there is no special mapping.
#[must_use]
pub fn telnet_to_display_source(code: &str) -> &str {
    // `IRP` is a second name of internet radio.
    if code == "IRP" {
        return "Internet Radio";
    }
    DISPLAY_NAMES
        .iter()
        .find(|(telnet, _)| *telnet == code)
        .map_or(code, |(_, display)| display)
}

/// Telnet command code to select a source by its display name.
#[must_use]
pub fn display_to_select_code(name: &str) -> Option<&'static str> {
    // Online music is selected as `NET`.
    if name == "Online Music" {
        return Some("NET");
    }
    DISPLAY_NAMES
        .iter()
        .find(|(_, display)| *display == name)
        .map(|(telnet, _)| *telnet)
}

/// Source names that receivers accept on telnet.
pub const TELNET_SOURCES: &[&str] = &[
    "CD",
    "PHONO",
    "TUNER",
    "DVD",
    "BD",
    "TV",
    "SAT/CBL",
    "MPLAY",
    "GAME",
    "HDRADIO",
    "NET",
    "PANDORA",
    "SIRIUSXM",
    "SOURCE",
    "LASTFM",
    "FLICKR",
    "IRADIO",
    "IRP",
    "SERVER",
    "FAVORITES",
    "AUX1",
    "AUX2",
    "AUX3",
    "AUX4",
    "AUX5",
    "AUX6",
    "AUX7",
    "BT",
    "USB/IPOD",
    "USB DIRECT",
    "IPOD DIRECT",
];

/// Returns true if receivers accept the name as a telnet source.
#[must_use]
pub fn is_telnet_source(name: &str) -> bool {
    TELNET_SOURCES.contains(&name)
}

/// Returns true for sources that play network audio. The name can be a
/// display name or a telnet name.
#[must_use]
pub fn is_netaudio_source(name: &str) -> bool {
    matches!(
        name,
        "AirPlay"
            | "Online Music"
            | "Media Server"
            | "iPod/USB"
            | "USB/IPOD"
            | "Bluetooth"
            | "BT"
            | "Internet Radio"
            | "IRADIO"
            | "IRP"
            | "SERVER"
            | "Favorites"
            | "FAVORITES"
            | "SpotifyConnect"
            | "Spotify Connect"
            | "Spotify"
            | "Flickr"
            | "FLICKR"
            | "NET/USB"
            | "Music Server"
            | "NETWORK"
            | "NET"
    )
}

/// Returns true for the tuner source.
#[must_use]
pub fn is_tuner_source(name: &str) -> bool {
    matches!(name, "Tuner" | "TUNER")
}

/// Returns true for the HD radio source.
#[must_use]
pub fn is_hd_tuner_source(name: &str) -> bool {
    matches!(name, "HD Radio" | "HDRADIO")
}

/// What a source plays, if it reports it. The name can be a display name or a
/// telnet name.
#[must_use]
pub fn playing_kind(name: &str) -> Option<PlayingKind> {
    if is_netaudio_source(name) {
        Some(PlayingKind::NetAudio)
    } else if is_tuner_source(name) {
        Some(PlayingKind::Tuner)
    } else if is_hd_tuner_source(name) {
        Some(PlayingKind::HdRadio)
    } else {
        None
    }
}

/// Returns true for sources that report what is playing.
#[must_use]
pub fn is_playing_source(name: &str) -> bool {
    playing_kind(name).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_names_map_to_telnet() {
        assert_eq!(xml_to_telnet_source("Blu-ray"), "BD");
        assert_eq!(xml_to_telnet_source("FM"), "TUNER");
        assert_eq!(xml_to_telnet_source("Game"), "Game");
    }

    #[test]
    fn telnet_names_map_to_display() {
        assert_eq!(telnet_to_display_source("IRP"), "Internet Radio");
        assert_eq!(telnet_to_display_source("CD"), "CD");
    }

    #[test]
    fn select_codes() {
        assert_eq!(display_to_select_code("Online Music"), Some("NET"));
        assert_eq!(display_to_select_code("CD"), None);
    }

    #[test]
    fn source_groups() {
        assert!(is_telnet_source("SAT/CBL"));
        assert!(!is_telnet_source("sat/cbl"));
        assert!(is_playing_source("TUNER"));
        assert!(is_playing_source("HDRADIO"));
        assert!(is_playing_source("NET"));
        assert!(!is_playing_source("CD"));
    }

    #[test]
    fn playing_kinds() {
        assert_eq!(playing_kind("USB/IPOD"), Some(PlayingKind::NetAudio));
        assert_eq!(playing_kind("Internet Radio"), Some(PlayingKind::NetAudio));
        assert_eq!(playing_kind("TUNER"), Some(PlayingKind::Tuner));
        assert_eq!(playing_kind("HD Radio"), Some(PlayingKind::HdRadio));
        assert_eq!(playing_kind("CD"), None);
    }
}
