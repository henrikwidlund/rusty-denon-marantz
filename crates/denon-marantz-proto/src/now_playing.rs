//! What the receiver plays: title, artist, album and station.
//!
//! Receivers report it in two ways. Telnet sends the lines of the display of
//! network audio as `NSE` events and the HD radio information as `HD` events.
//! Receivers also have status pages with the same values. A receiver that is
//! new enough to use HEOS for network audio gives neither.

use crate::error::Error;
use crate::speaker_tables::root;
use crate::xml::Node;

/// Path of the status page of network audio.
pub const NET_AUDIO_STATUS_PATH: &str = "/goform/formNetAudio_StatusXml.xml";

/// Path of the status page of HD radio.
pub const HD_RADIO_STATUS_PATH: &str = "/goform/formTuner_HdXml.xml";

/// Path of the picture that is shown while the tuner or HD radio plays, and
/// for a source that has no picture.
pub const STATIC_ARTWORK_PATH: &str = "/img/album%20art_S.png";

/// Path of the cover of the title that plays on network audio.
pub const COVER_ARTWORK_PATH: &str = "/NetAudio/art.asp-jpg";

/// The text of the first line of network audio while a title plays.
const PLAYING_TEXT: &str = "Now Playing";

/// What a source plays, and which page or events tell it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PlayingKind {
    /// Network audio, for example internet radio, USB or Bluetooth.
    NetAudio,
    /// The tuner. Its values are in [`TunerState`](crate::TunerState).
    Tuner,
    /// HD radio.
    HdRadio,
}

/// What is playing. A value is `None` if the receiver does not report it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NowPlaying {
    /// Text of the first line of network audio, for example `Now Playing`.
    pub status: Option<String>,
    /// Title. For internet radio it can have the artist too.
    pub title: Option<String>,
    /// Artist. For internet radio it can be the name of the station.
    pub artist: Option<String>,
    /// Album.
    pub album: Option<String>,
    /// Name of the station, for HD radio.
    pub station: Option<String>,
}

impl NowPlaying {
    /// Returns true if nothing is known.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.status.is_none()
            && self.title.is_none()
            && self.artist.is_none()
            && self.album.is_none()
            && self.station.is_none()
    }

    /// Returns true if network audio plays a title, `None` if the receiver has
    /// not said. The receiver does not tell a pause from a stop.
    #[must_use]
    pub fn is_playing(&self) -> Option<bool> {
        self.status
            .as_deref()
            .map(|status| status.starts_with(PLAYING_TEXT))
    }

    /// Reads the status page of network audio.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML or is an HTML page.
    pub fn parse_net_audio(body: &[u8]) -> Result<Self, Error> {
        let node = root(body)?.node();
        let lines = node.child("szLine").map(|lines| lines.node());
        let line = |index: usize| {
            lines
                .and_then(|lines| lines.children().filter(|c| c.is("value")).nth(index))
                .and_then(|value| clean(&value.node().text()?))
        };
        Ok(Self {
            status: line(0),
            title: line(1),
            artist: line(2),
            album: line(4),
            station: None,
        })
    }

    /// Reads the status page of HD radio.
    ///
    /// # Errors
    ///
    /// Returns an error if the body is not valid XML or is an HTML page.
    pub fn parse_hd_radio(body: &[u8]) -> Result<Self, Error> {
        let node = root(body)?.node();
        Ok(Self {
            status: None,
            title: value_of(node, "Title"),
            artist: value_of(node, "Artist"),
            album: value_of(node, "Album"),
            station: value_of(node, "StationNameSh"),
        })
    }

    /// Applies the text of an `NSE` event, for example `1Song title`. The
    /// first character is the number of the line of the display. Returns
    /// `None` for a line that is not used, and whether the state changed
    /// otherwise.
    pub(crate) fn apply_net_audio_line(&mut self, parameter: &str) -> Option<bool> {
        let mut chars = parameter.chars();
        let number = chars.next()?;
        let text = clean(chars.as_str());
        let field = match number {
            '0' => &mut self.status,
            '1' => &mut self.title,
            '2' => &mut self.artist,
            '4' => &mut self.album,
            _ => return None,
        };
        Some(replace(field, text))
    }

    /// Applies the text of an `HD` event that is not about zone 2 or 3, for
    /// example `ARTISTName`. Returns `None` for text that is not used, and
    /// whether the state changed otherwise.
    pub(crate) fn apply_hd_radio(&mut self, parameter: &str) -> Option<bool> {
        let (field, rest) = [
            ("ARTIST", &mut self.artist),
            ("TITLE", &mut self.title),
            ("ALBUM", &mut self.album),
            ("ST NAME", &mut self.station),
        ]
        .into_iter()
        .find_map(|(prefix, field)| Some((field, parameter.strip_prefix(prefix)?)))?;
        Some(replace(field, clean(rest)))
    }
}

/// Text of the `value` element in the element `tag`.
fn value_of(node: Node<'_>, tag: &str) -> Option<String> {
    let value = node.child(tag)?.node().child("value")?;
    clean(&value.node().text()?)
}

/// Removes control characters and the space around the text. `None` if
/// nothing is left. Receivers send a control character at the start of some
/// lines.
fn clean(text: &str) -> Option<String> {
    let text: String = text.chars().filter(|c| !c.is_control()).collect();
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn replace(field: &mut Option<String>, value: Option<String>) -> bool {
    if *field == value {
        false
    } else {
        *field = value;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYING: &[u8] = include_bytes!("../tests/data/xml/AVR-3312-formNetAudio_StatusXml.xml");
    const RADIO: &[u8] = include_bytes!("../tests/data/xml/AVR-X2100W-formNetAudio_StatusXml.xml");
    const BROWSING: &[u8] =
        include_bytes!("../tests/data/xml/AVR-X4000-formNetAudio_StatusXml.xml");
    const STOPPED: &[u8] =
        include_bytes!("../tests/data/xml/Own-formNetAudio_StatusXml-stopped.xml");
    const HD: &[u8] = include_bytes!("../tests/data/xml/Own-formTuner_HdXml-playing.xml");
    const HD_EMPTY: &[u8] = include_bytes!("../tests/data/xml/AVR-3312-formTuner_HdXml.xml");

    #[test]
    fn network_audio_page_with_a_title() {
        let now = NowPlaying::parse_net_audio(PLAYING).unwrap();
        assert_eq!(now.status.as_deref(), Some("Now Playing"));
        assert_eq!(
            now.title.as_deref(),
            Some("Moetley Crüe - Girls, girls, girls")
        );
        assert_eq!(now.artist, None);
        assert_eq!(now.album, None);
        assert_eq!(now.is_playing(), Some(true));
    }

    #[test]
    fn internet_radio_page_has_the_station_as_artist() {
        let now = NowPlaying::parse_net_audio(RADIO).unwrap();
        assert_eq!(
            now.title.as_deref(),
            Some("Sia - The Greatest (feat. Kendrick Lamar)")
        );
        assert_eq!(now.artist.as_deref(), Some("1.fm Absolute TOP 40"));
        // The album is the fifth line, and this page has text in the sixth.
        assert_eq!(now.album, None);
    }

    #[test]
    fn browsing_and_stopped_pages_are_not_playing() {
        let browsing = NowPlaying::parse_net_audio(BROWSING).unwrap();
        assert_eq!(browsing.is_playing(), Some(false));
        let stopped = NowPlaying::parse_net_audio(STOPPED).unwrap();
        assert_eq!(stopped.status.as_deref(), Some("Stopped"));
        assert_eq!(stopped.is_playing(), Some(false));
        assert_eq!(NowPlaying::default().is_playing(), None);
    }

    #[test]
    fn hd_radio_page() {
        let now = NowPlaying::parse_hd_radio(HD).unwrap();
        assert_eq!(now.title.as_deref(), Some("Example Title"));
        assert_eq!(now.artist.as_deref(), Some("Example Artist"));
        assert_eq!(now.album.as_deref(), Some("Example Album"));
        assert_eq!(now.station.as_deref(), Some("WXYZ-HD2"));
        assert!(NowPlaying::parse_hd_radio(HD_EMPTY).unwrap().is_empty());
    }

    #[test]
    fn html_is_an_error() {
        assert!(NowPlaying::parse_net_audio(b"<html><body>no</body></html>").is_err());
        assert!(NowPlaying::parse_hd_radio(b"<html><body>no</body></html>").is_err());
    }

    #[test]
    fn display_lines_change_one_value_each() {
        let mut now = NowPlaying::default();
        assert_eq!(now.apply_net_audio_line("0Now Playing"), Some(true));
        assert_eq!(now.apply_net_audio_line("0Now Playing"), Some(false));
        assert_eq!(now.apply_net_audio_line("1\u{1}Song  "), Some(true));
        assert_eq!(now.apply_net_audio_line("2Band"), Some(true));
        assert_eq!(now.apply_net_audio_line("4Record"), Some(true));
        // Other lines of the display are not used.
        assert_eq!(now.apply_net_audio_line("3Bitrate"), None);
        assert_eq!(now.apply_net_audio_line(""), None);
        assert_eq!(now.title.as_deref(), Some("Song"));
        assert_eq!(now.artist.as_deref(), Some("Band"));
        assert_eq!(now.album.as_deref(), Some("Record"));
        assert_eq!(now.is_playing(), Some(true));
        // An empty line clears the value.
        assert_eq!(now.apply_net_audio_line("2"), Some(true));
        assert_eq!(now.artist, None);
    }

    #[test]
    fn hd_radio_events_change_one_value_each() {
        let mut now = NowPlaying::default();
        assert_eq!(now.apply_hd_radio("ARTISTBand"), Some(true));
        assert_eq!(now.apply_hd_radio("TITLESong"), Some(true));
        assert_eq!(now.apply_hd_radio("ALBUMRecord"), Some(true));
        assert_eq!(now.apply_hd_radio("ST NAMEWXYZ"), Some(true));
        assert_eq!(now.apply_hd_radio("ST NAMEWXYZ"), Some(false));
        assert_eq!(now.apply_hd_radio("A PCM"), None);
        assert_eq!(now.station.as_deref(), Some("WXYZ"));
    }
}
