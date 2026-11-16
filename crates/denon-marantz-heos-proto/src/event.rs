//! Change events.

use std::borrow::Cow;

use crate::error::Error;
use crate::message::Message;
use crate::types::{PlayState, PlayerId, Progress, RepeatMode, owned};

/// A change event. Events come only after `register_for_change_events`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event<'a> {
    /// The music sources changed.
    SourcesChanged,
    /// Players were added or removed.
    PlayersChanged,
    /// Groups changed.
    GroupsChanged,
    /// The play state changed.
    PlayerStateChanged {
        /// Player.
        pid: PlayerId,
        /// New state, or `None` if the device sent `unknown`.
        state: Option<PlayState>,
    },
    /// What plays changed. The event has no details: ask with
    /// `get_now_playing_media`. It can come several times in a row.
    NowPlayingChanged {
        /// Player.
        pid: PlayerId,
    },
    /// Position in what plays. Comes every few seconds while playing.
    NowPlayingProgress {
        /// Player.
        pid: PlayerId,
        /// Position and length.
        progress: Progress,
    },
    /// Playback failed.
    PlaybackError {
        /// Player.
        pid: PlayerId,
        /// Error text that can be shown to the user.
        error: Cow<'a, str>,
    },
    /// The queue changed.
    QueueChanged {
        /// Player.
        pid: PlayerId,
    },
    /// The repeat mode changed.
    RepeatModeChanged {
        /// Player.
        pid: PlayerId,
        /// New mode.
        repeat: RepeatMode,
    },
    /// Shuffle was turned on or off.
    ShuffleModeChanged {
        /// Player.
        pid: PlayerId,
        /// Shuffle is on.
        shuffle: bool,
    },
    /// The HEOS account signed in or out.
    UserChanged {
        /// User name, or `None` if signed out.
        user: Option<Cow<'a, str>>,
    },
    /// An event that this crate does not parse. The volume events of
    /// players and groups also come here.
    Unknown {
        /// Name without the `event/` prefix.
        name: Cow<'a, str>,
        /// The message as received.
        message: Cow<'a, str>,
    },
}

impl<'a> Event<'a> {
    /// Parses an event from its name without the `event/` prefix and its
    /// message.
    pub fn parse(name: &'a str, message: Message<'a>) -> Result<Self, Error> {
        let pid = || message.parse::<PlayerId>("pid");
        Ok(match name {
            "sources_changed" => Self::SourcesChanged,
            "players_changed" => Self::PlayersChanged,
            "groups_changed" => Self::GroupsChanged,
            "player_state_changed" => Self::PlayerStateChanged {
                pid: pid()?,
                state: message.play_state()?,
            },
            "player_now_playing_changed" => Self::NowPlayingChanged { pid: pid()? },
            "player_now_playing_progress" => Self::NowPlayingProgress {
                pid: pid()?,
                progress: Progress {
                    position_ms: message.parse("cur_pos")?,
                    duration_ms: message.parse("duration")?,
                },
            },
            "player_playback_error" => Self::PlaybackError {
                pid: pid()?,
                error: message.get("error").unwrap_or_default(),
            },
            "player_queue_changed" => Self::QueueChanged { pid: pid()? },
            "repeat_mode_changed" => Self::RepeatModeChanged {
                pid: pid()?,
                repeat: message.parse_with("repeat", RepeatMode::from_name)?,
            },
            "shuffle_mode_changed" => Self::ShuffleModeChanged {
                pid: pid()?,
                shuffle: message.on_off("shuffle")?,
            },
            "user_changed" => Self::UserChanged {
                user: if message.has("signed_in") {
                    message.get("un")
                } else {
                    None
                },
            },
            _ => Self::Unknown {
                name: Cow::Borrowed(name),
                message: Cow::Borrowed(message.as_str()),
            },
        })
    }

    /// The player of a player event.
    #[must_use]
    pub const fn pid(&self) -> Option<PlayerId> {
        match self {
            Self::PlayerStateChanged { pid, .. }
            | Self::NowPlayingChanged { pid }
            | Self::NowPlayingProgress { pid, .. }
            | Self::PlaybackError { pid, .. }
            | Self::QueueChanged { pid }
            | Self::RepeatModeChanged { pid, .. }
            | Self::ShuffleModeChanged { pid, .. } => Some(*pid),
            _ => None,
        }
    }

    /// Copies the borrowed text.
    #[must_use]
    pub fn into_owned(self) -> Event<'static> {
        match self {
            Self::SourcesChanged => Event::SourcesChanged,
            Self::PlayersChanged => Event::PlayersChanged,
            Self::GroupsChanged => Event::GroupsChanged,
            Self::PlayerStateChanged { pid, state } => Event::PlayerStateChanged { pid, state },
            Self::NowPlayingChanged { pid } => Event::NowPlayingChanged { pid },
            Self::NowPlayingProgress { pid, progress } => {
                Event::NowPlayingProgress { pid, progress }
            }
            Self::PlaybackError { pid, error } => Event::PlaybackError {
                pid,
                error: owned(error),
            },
            Self::QueueChanged { pid } => Event::QueueChanged { pid },
            Self::RepeatModeChanged { pid, repeat } => Event::RepeatModeChanged { pid, repeat },
            Self::ShuffleModeChanged { pid, shuffle } => Event::ShuffleModeChanged { pid, shuffle },
            Self::UserChanged { user } => Event::UserChanged {
                user: user.map(owned),
            },
            Self::Unknown { name, message } => Event::Unknown {
                name: owned(name),
                message: owned(message),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse<'a>(name: &'a str, message: &'a str) -> Result<Event<'a>, Error> {
        Event::parse(name, Message::new(message))
    }

    #[test]
    fn parses_spec_events() {
        let pid = PlayerId(-5);
        let cases = [
            ("sources_changed", "", Event::SourcesChanged),
            ("players_changed", "", Event::PlayersChanged),
            ("groups_changed", "", Event::GroupsChanged),
            (
                "player_state_changed",
                "pid=-5&state=pause",
                Event::PlayerStateChanged {
                    pid,
                    state: Some(PlayState::Pause),
                },
            ),
            (
                "player_now_playing_changed",
                "pid=-5",
                Event::NowPlayingChanged { pid },
            ),
            (
                "player_now_playing_progress",
                "pid=-5&cur_pos=1000&duration=0",
                Event::NowPlayingProgress {
                    pid,
                    progress: Progress {
                        position_ms: 1000,
                        duration_ms: 0,
                    },
                },
            ),
            (
                "player_playback_error",
                "pid=-5&error=Could Not Download",
                Event::PlaybackError {
                    pid,
                    error: "Could Not Download".into(),
                },
            ),
            (
                "player_queue_changed",
                "pid=-5",
                Event::QueueChanged { pid },
            ),
            (
                "repeat_mode_changed",
                "pid=-5&repeat=on_one",
                Event::RepeatModeChanged {
                    pid,
                    repeat: RepeatMode::OnOne,
                },
            ),
            (
                "shuffle_mode_changed",
                "pid=-5&shuffle=on",
                Event::ShuffleModeChanged { pid, shuffle: true },
            ),
            (
                "user_changed",
                "signed_in&un=user@example.com",
                Event::UserChanged {
                    user: Some("user@example.com".into()),
                },
            ),
            (
                "user_changed",
                "signed_out",
                Event::UserChanged { user: None },
            ),
        ];
        for (name, message, expected) in cases {
            let event = parse(name, message).unwrap();
            assert_eq!(event, expected, "{name}");
            assert_eq!(event.clone().into_owned(), expected, "{name}");
        }
    }

    #[test]
    fn unknown_event_keeps_its_name() {
        for name in ["player_volume_changed", "group_volume_changed"] {
            assert!(matches!(
                parse(name, "pid=1&level=37&mute=off"),
                Ok(Event::Unknown { .. })
            ));
        }
        let event = parse("new_event", "pid=1").unwrap();
        assert_eq!(
            event,
            Event::Unknown {
                name: "new_event".into(),
                message: "pid=1".into(),
            }
        );
        assert_eq!(event.pid(), None);
    }

    #[test]
    fn unknown_play_state_is_none() {
        assert_eq!(
            parse("player_state_changed", "pid=1&state=unknown"),
            Ok(Event::PlayerStateChanged {
                pid: PlayerId(1),
                state: None,
            })
        );
    }

    #[test]
    fn missing_or_bad_fields_are_errors() {
        assert_eq!(
            parse("player_state_changed", "state=play"),
            Err(Error::MissingField { field: "pid" })
        );
        assert_eq!(
            parse("player_state_changed", "pid=1&state=rewind"),
            Err(Error::InvalidValue { field: "state" })
        );
        assert_eq!(
            parse("player_now_playing_progress", "pid=1&cur_pos=1x&duration=0"),
            Err(Error::InvalidValue { field: "cur_pos" })
        );
        assert_eq!(
            parse("shuffle_mode_changed", "pid=1&shuffle=yes"),
            Err(Error::InvalidValue { field: "shuffle" })
        );
    }

    #[test]
    fn playback_error_text_is_decoded() {
        let event = parse("player_playback_error", "pid=1&error=A %26 B").unwrap();
        assert_eq!(
            event,
            Event::PlaybackError {
                pid: PlayerId(1),
                error: "A & B".into(),
            }
        );
        assert_eq!(event.pid(), Some(PlayerId(1)));
    }
}
