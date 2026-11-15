//! Transport controls that the CLI allows for a source and media type.

use crate::types::{MediaType, SourceId};

/// Transport controls that work for what plays.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[allow(clippy::struct_excessive_bools)] // One flag for each control.
pub struct Controls {
    /// Play.
    pub play: bool,
    /// Pause.
    pub pause: bool,
    /// Stop.
    pub stop: bool,
    /// Next item.
    pub next: bool,
    /// Previous item.
    pub previous: bool,
}

impl Controls {
    /// All controls.
    pub const ALL: Self = Self {
        play: true,
        pause: true,
        stop: true,
        next: true,
        previous: true,
    };
    /// Play, pause, stop and next.
    const NO_PREVIOUS: Self = Self {
        previous: false,
        ..Self::ALL
    };
    /// Play and stop.
    const PLAY_STOP: Self = Self {
        pause: false,
        next: false,
        ..Self::NO_PREVIOUS
    };

    /// Controls for a source and media type, from the table in the
    /// specification.
    ///
    /// Only some stations have fewer controls. An unknown source, a song, or a
    /// station that the table has no entry for allows all controls. `AirPlay`
    /// is a station of [`SourceId::LOCAL_MUSIC`] and allows all controls.
    #[must_use]
    pub fn for_media(sid: Option<SourceId>, media_type: MediaType) -> Self {
        if media_type != MediaType::Station {
            return Self::ALL;
        }
        match sid {
            Some(SourceId::DEEZER | SourceId::NAPSTER | SourceId::PANDORA | SourceId::RHAPSODY) => {
                Self::NO_PREVIOUS
            }
            Some(
                SourceId::IHEARTRADIO
                | SourceId::SIRIUS_XM
                | SourceId::TUNEIN
                | SourceId::AUX_INPUT,
            ) => Self::PLAY_STOP,
            _ => Self::ALL,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stations_with_fewer_controls() {
        let tunein = Controls::for_media(Some(SourceId::TUNEIN), MediaType::Station);
        assert_eq!(tunein, Controls::PLAY_STOP);
        assert!(!tunein.pause && !tunein.next);
        assert_eq!(
            Controls::for_media(Some(SourceId::DEEZER), MediaType::Station),
            Controls::NO_PREVIOUS
        );
    }

    #[test]
    fn songs_and_unknown_sources_allow_all() {
        assert_eq!(
            Controls::for_media(Some(SourceId::TUNEIN), MediaType::Song),
            Controls::ALL
        );
        assert_eq!(
            Controls::for_media(Some(SourceId::LOCAL_MUSIC), MediaType::Station),
            Controls::ALL
        );
        assert_eq!(
            Controls::for_media(Some(SourceId(0)), MediaType::Station),
            Controls::ALL
        );
        assert_eq!(Controls::for_media(None, MediaType::Other), Controls::ALL);
    }
}
