//! Values of the HEOS CLI.

use std::borrow::Cow;
use std::fmt;
use std::net::IpAddr;
use std::str::FromStr;

use serde::{Deserialize, Deserializer};

use crate::de::{self, Number};

/// Id of a player (`pid`) or of a group (`gid`). It can be negative.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PlayerId(pub i64);

/// Id of a music source (`sid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceId(pub u32);

/// Id of an item in the queue of a player (`qid`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QueueId(pub u64);

macro_rules! id_impls {
    ($($name:ident($inner:ty)),*) => {$(
        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(formatter)
            }
        }

        /// Spaces around the number are ignored.
        impl FromStr for $name {
            type Err = std::num::ParseIntError;

            fn from_str(text: &str) -> Result<Self, Self::Err> {
                text.trim().parse().map(Self)
            }
        }

        impl Number for $name {
            fn from_i64(value: i64) -> Option<Self> {
                <$inner>::from_i64(value).map(Self)
            }

            fn from_u64(value: u64) -> Option<Self> {
                <$inner>::from_u64(value).map(Self)
            }

            fn from_text(text: &str) -> Option<Self> {
                <$inner>::from_text(text).map(Self)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                de::number(deserializer)
            }
        }
    )*};
}

id_impls!(PlayerId(i64), SourceId(u32), QueueId(u64));

impl SourceId {
    /// Pandora.
    pub const PANDORA: Self = Self(1);
    /// Rhapsody.
    pub const RHAPSODY: Self = Self(2);
    /// `TuneIn`.
    pub const TUNEIN: Self = Self(3);
    /// Spotify.
    pub const SPOTIFY: Self = Self(4);
    /// Deezer.
    pub const DEEZER: Self = Self(5);
    /// Napster.
    pub const NAPSTER: Self = Self(6);
    /// iHeartRadio.
    pub const IHEARTRADIO: Self = Self(7);
    /// Sirius XM.
    pub const SIRIUS_XM: Self = Self(8);
    /// `SoundCloud`.
    pub const SOUNDCLOUD: Self = Self(9);
    /// Tidal.
    pub const TIDAL: Self = Self(10);
    /// Amazon Music.
    pub const AMAZON_MUSIC: Self = Self(13);
    /// Qobuz. Not in the specification.
    pub const QOBUZ: Self = Self(30);
    /// Local USB media and DLNA servers. `AirPlay` also uses this id.
    pub const LOCAL_MUSIC: Self = Self(1024);
    /// HEOS playlists.
    pub const PLAYLISTS: Self = Self(1025);
    /// HEOS history.
    pub const HISTORY: Self = Self(1026);
    /// HEOS aux inputs.
    pub const AUX_INPUT: Self = Self(1027);
    /// HEOS Favorites.
    pub const FAVORITES: Self = Self(1028);
}

macro_rules! name_enum {
    ($(#[$meta:meta])* $name:ident { $($(#[$variant_meta:meta])* $variant:ident = $text:literal,)* }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($(#[$variant_meta])* $variant,)*
        }

        impl $name {
            /// The name that the CLI uses.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)*
                }
            }

            /// Parses the name that the CLI uses.
            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                match name {
                    $($text => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

name_enum!(
    /// Play state of a player.
    PlayState {
        /// Playing.
        Play = "play",
        /// Paused.
        Pause = "pause",
        /// Stopped. Pausing a station that cannot pause gives this state.
        Stop = "stop",
    }
);

name_enum!(
    /// Repeat mode of a player.
    RepeatMode {
        /// No repeat.
        Off = "off",
        /// Repeat the whole queue.
        OnAll = "on_all",
        /// Repeat one item.
        OnOne = "on_one",
    }
);

/// Media type of what is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
#[non_exhaustive]
pub enum MediaType {
    /// A track.
    Song,
    /// A station. `AirPlay` and inputs are also stations.
    Station,
    /// Any other type.
    #[serde(other)]
    Other,
}

/// Position in what is playing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Progress {
    /// Position in milliseconds.
    pub position_ms: u64,
    /// Length in milliseconds. It is 0 for a live station.
    pub duration_ms: u64,
}

/// A player from `get_players` or `get_player_info`.
///
/// Fields other than `name` and `pid` can be missing. An optional number that
/// is not valid is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Player<'a> {
    /// Name.
    #[serde(borrow, deserialize_with = "de::text")]
    pub name: Cow<'a, str>,
    /// Player id.
    pub pid: PlayerId,
    /// Id of the group leader, if the player is in a group.
    #[serde(default, deserialize_with = "de::lenient_number")]
    pub gid: Option<PlayerId>,
    /// Model.
    #[serde(default, borrow, deserialize_with = "de::option_text")]
    pub model: Option<Cow<'a, str>>,
    /// Firmware version.
    #[serde(default, borrow, deserialize_with = "de::option_text")]
    pub version: Option<Cow<'a, str>>,
    /// IP address as text.
    #[serde(default, borrow, deserialize_with = "de::option_text")]
    pub ip: Option<Cow<'a, str>>,
    /// Network type: `wired`, `wifi` or `unknown`.
    #[serde(default, borrow, deserialize_with = "de::option_text")]
    pub network: Option<Cow<'a, str>>,
    /// Line out level type: 1 variable, 2 fixed.
    #[serde(default, deserialize_with = "de::lenient_number")]
    pub lineout: Option<u8>,
    /// Line out control when the level is fixed: 1 none, 2 IR, 3 trigger,
    /// 4 network.
    #[serde(default, deserialize_with = "de::lenient_number")]
    pub control: Option<u8>,
    /// Serial number.
    #[serde(default, borrow, deserialize_with = "de::option_text")]
    pub serial: Option<Cow<'a, str>>,
}

impl Player<'_> {
    /// The IP address, if it is present and valid.
    #[must_use]
    pub fn ip_addr(&self) -> Option<IpAddr> {
        self.ip.as_deref()?.parse().ok()
    }

    /// Copies the borrowed text.
    #[must_use]
    pub fn into_owned(self) -> Player<'static> {
        Player {
            name: owned(self.name),
            pid: self.pid,
            gid: self.gid,
            model: self.model.map(owned),
            version: self.version.map(owned),
            ip: self.ip.map(owned),
            network: self.network.map(owned),
            lineout: self.lineout,
            control: self.control,
            serial: self.serial.map(owned),
        }
    }
}

/// What a player plays, from `get_now_playing_media`.
///
/// After a stop, the CLI can still send the last item. An optional number that
/// is not valid is `None`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NowPlaying<'a> {
    /// Media type.
    #[serde(rename = "type")]
    pub media_type: MediaType,
    /// Track title. For a station it can be the stream description.
    #[serde(default, borrow, deserialize_with = "de::text")]
    pub song: Cow<'a, str>,
    /// Station name, for a station.
    #[serde(default, borrow, deserialize_with = "de::option_text")]
    pub station: Option<Cow<'a, str>>,
    /// Album.
    #[serde(default, borrow, deserialize_with = "de::text")]
    pub album: Cow<'a, str>,
    /// Artist.
    #[serde(default, borrow, deserialize_with = "de::text")]
    pub artist: Cow<'a, str>,
    /// URL of the cover image. It can be empty.
    #[serde(default, borrow, deserialize_with = "de::text")]
    pub image_url: Cow<'a, str>,
    /// Album id.
    #[serde(default, borrow, deserialize_with = "de::option_text")]
    pub album_id: Option<Cow<'a, str>>,
    /// Media id.
    #[serde(default, borrow, deserialize_with = "de::option_text")]
    pub mid: Option<Cow<'a, str>>,
    /// Queue id.
    #[serde(default, deserialize_with = "de::lenient_number")]
    pub qid: Option<QueueId>,
    /// Source id.
    #[serde(default, deserialize_with = "de::lenient_number")]
    pub sid: Option<SourceId>,
}

impl NowPlaying<'_> {
    /// Copies the borrowed text.
    #[must_use]
    pub fn into_owned(self) -> NowPlaying<'static> {
        NowPlaying {
            media_type: self.media_type,
            song: owned(self.song),
            station: self.station.map(owned),
            album: owned(self.album),
            artist: owned(self.artist),
            image_url: owned(self.image_url),
            album_id: self.album_id.map(owned),
            mid: self.mid.map(owned),
            qid: self.qid,
            sid: self.sid,
        }
    }
}

/// A quick select slot from `get_quickselects`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct QuickSelect<'a> {
    /// Slot, from 1.
    #[serde(deserialize_with = "de::number")]
    pub id: u8,
    /// Name.
    #[serde(borrow, deserialize_with = "de::text")]
    pub name: Cow<'a, str>,
}

impl QuickSelect<'_> {
    /// Copies the borrowed text.
    #[must_use]
    pub fn into_owned(self) -> QuickSelect<'static> {
        QuickSelect {
            id: self.id,
            name: owned(self.name),
        }
    }
}

pub(crate) fn owned(text: Cow<'_, str>) -> Cow<'static, str> {
    Cow::Owned(text.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_numbers_that_are_not_valid_are_none() {
        let now: NowPlaying<'_> =
            serde_json::from_str(r#"{"type":"song","qid":"","sid":-1,"mid":7,"x":{"a":[1]}}"#)
                .unwrap();
        assert_eq!((now.qid, now.sid), (None, None));
        assert_eq!(now.mid.as_deref(), Some("7"));

        let player: Player<'_> = serde_json::from_str(
            r#"{"name":"A","pid":"-3","gid":[1],"lineout":300,"control":null}"#,
        )
        .unwrap();
        assert_eq!(player.pid, PlayerId(-3));
        assert_eq!(
            (player.gid, player.lineout, player.control),
            (None, None, None)
        );
    }

    #[test]
    fn required_numbers_must_be_valid() {
        for line in [
            r#"{"name":"A","pid":""}"#,
            r#"{"name":"A","pid":null}"#,
            r#"{"name":"A","pid":1.5}"#,
            r#"{"name":"A"}"#,
        ] {
            assert!(serde_json::from_str::<Player<'_>>(line).is_err(), "{line}");
        }
        assert!(serde_json::from_str::<QuickSelect<'_>>(r#"{"id":256,"name":"A"}"#).is_err());
    }

    #[test]
    fn ids_from_text_ignore_spaces() {
        assert_eq!(" -7 ".parse(), Ok(PlayerId(-7)));
        assert_eq!("1028".parse(), Ok(SourceId::FAVORITES));
        assert!("".parse::<QueueId>().is_err());
        let player: Player<'_> = serde_json::from_str(r#"{"name":"A","pid":" 5 "}"#).unwrap();
        assert_eq!(player.pid, PlayerId(5));
    }
}
