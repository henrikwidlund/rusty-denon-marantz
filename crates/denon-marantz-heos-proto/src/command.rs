//! Commands to the CLI.

use std::fmt::Write as _;

use crate::error::Error;
use crate::message::push_encoded;
use crate::types::{PlayState, PlayerId, QueueId, RepeatMode, SourceId};

/// Highest quick select slot.
const MAX_QUICK_SELECT: u8 = 6;

/// A command to the CLI.
///
/// Volume, mute, groups, sign-in and service browsing are not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command<'a> {
    /// Turns change events on or off for this connection.
    RegisterForChangeEvents {
        /// Send events.
        enable: bool,
    },
    /// Checks that the connection works.
    HeartBeat,
    /// Lists the players.
    GetPlayers,
    /// Reads one player.
    GetPlayerInfo {
        /// Player.
        pid: PlayerId,
    },
    /// Reads the play state.
    GetPlayState {
        /// Player.
        pid: PlayerId,
    },
    /// Plays, pauses or stops.
    SetPlayState {
        /// Player.
        pid: PlayerId,
        /// New state.
        state: PlayState,
    },
    /// Reads what plays.
    GetNowPlayingMedia {
        /// Player.
        pid: PlayerId,
    },
    /// Reads the repeat and shuffle modes.
    GetPlayMode {
        /// Player.
        pid: PlayerId,
    },
    /// Sets the repeat and shuffle modes.
    SetPlayMode {
        /// Player.
        pid: PlayerId,
        /// Repeat mode.
        repeat: RepeatMode,
        /// Shuffle on.
        shuffle: bool,
    },
    /// Plays the next item.
    PlayNext {
        /// Player.
        pid: PlayerId,
    },
    /// Plays the previous item.
    PlayPrevious {
        /// Player.
        pid: PlayerId,
    },
    /// Reads the queue, at most 100 items per answer.
    GetQueue {
        /// Player.
        pid: PlayerId,
        /// First and last index, from 0. `None` reads from the start.
        range: Option<(u32, u32)>,
    },
    /// Plays an item of the queue.
    PlayQueue {
        /// Player.
        pid: PlayerId,
        /// Item.
        qid: QueueId,
    },
    /// Removes items from the queue.
    RemoveFromQueue {
        /// Player.
        pid: PlayerId,
        /// Items. Must not be empty.
        qids: &'a [QueueId],
    },
    /// Removes all items from the queue.
    ClearQueue {
        /// Player.
        pid: PlayerId,
    },
    /// Reads the names of the quick select slots.
    GetQuickSelects {
        /// Player.
        pid: PlayerId,
    },
    /// Plays a quick select slot.
    PlayQuickSelect {
        /// Player.
        pid: PlayerId,
        /// Slot from 1 to 6.
        id: u8,
    },
    /// Lists the HEOS Favorites (`browse` of [`SourceId::FAVORITES`]).
    GetFavorites {
        /// First and last index, from 0. `None` reads from the start.
        range: Option<(u32, u32)>,
    },
    /// Plays a HEOS Favorite.
    PlayPreset {
        /// Player.
        pid: PlayerId,
        /// Position in the favorites, from 1.
        preset: u32,
    },
    /// Plays an input, for example `inputs/aux_in_1`.
    PlayInput {
        /// Player that plays.
        pid: PlayerId,
        /// Player that has the input, if it is another player.
        source: Option<PlayerId>,
        /// Input name.
        input: &'a str,
    },
    /// Plays a stream from a URL.
    PlayStream {
        /// Player.
        pid: PlayerId,
        /// Absolute URL.
        url: &'a str,
    },
}

impl Command<'_> {
    /// Name of the command, the same as in its answer.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::RegisterForChangeEvents { .. } => "system/register_for_change_events",
            Self::HeartBeat => "system/heart_beat",
            Self::GetPlayers => "player/get_players",
            Self::GetPlayerInfo { .. } => "player/get_player_info",
            Self::GetPlayState { .. } => "player/get_play_state",
            Self::SetPlayState { .. } => "player/set_play_state",
            Self::GetNowPlayingMedia { .. } => "player/get_now_playing_media",
            Self::GetPlayMode { .. } => "player/get_play_mode",
            Self::SetPlayMode { .. } => "player/set_play_mode",
            Self::PlayNext { .. } => "player/play_next",
            Self::PlayPrevious { .. } => "player/play_previous",
            Self::GetQueue { .. } => "player/get_queue",
            Self::PlayQueue { .. } => "player/play_queue",
            Self::RemoveFromQueue { .. } => "player/remove_from_queue",
            Self::ClearQueue { .. } => "player/clear_queue",
            Self::GetQuickSelects { .. } => "player/get_quickselects",
            Self::PlayQuickSelect { .. } => "player/play_quickselect",
            Self::GetFavorites { .. } => "browse/browse",
            Self::PlayPreset { .. } => "browse/play_preset",
            Self::PlayInput { .. } => "browse/play_input",
            Self::PlayStream { .. } => "browse/play_stream",
        }
    }

    /// Appends the command line, with `\r\n`, to `out`.
    ///
    /// On an error, `out` is not changed.
    pub fn write(&self, out: &mut String) -> Result<(), Error> {
        self.validate()?;
        out.push_str("heos://");
        out.push_str(self.name());
        // Writing to a `String` does not fail.
        let _ = self.write_arguments(out);
        out.push_str("\r\n");
        Ok(())
    }

    fn validate(&self) -> Result<(), Error> {
        match *self {
            Self::GetQueue {
                range: Some((first, last)),
                ..
            }
            | Self::GetFavorites {
                range: Some((first, last)),
            } if first > last => Err(Error::ValueOutOfRange { what: "range" }),
            Self::RemoveFromQueue { qids: [], .. } => {
                Err(Error::ValueOutOfRange { what: "queue ids" })
            }
            Self::PlayQuickSelect { id, .. } if !(1..=MAX_QUICK_SELECT).contains(&id) => {
                Err(Error::ValueOutOfRange {
                    what: "quick select",
                })
            }
            Self::PlayPreset { preset: 0, .. } => Err(Error::ValueOutOfRange { what: "preset" }),
            Self::PlayInput { input: text, .. } | Self::PlayStream { url: text, .. }
                if text.is_empty() || text.chars().any(char::is_control) =>
            {
                Err(Error::InvalidCommandText)
            }
            _ => Ok(()),
        }
    }

    fn write_arguments(&self, out: &mut String) -> std::fmt::Result {
        match *self {
            Self::RegisterForChangeEvents { enable } => {
                write!(out, "?enable={}", on_off(enable))
            }
            Self::HeartBeat | Self::GetPlayers => Ok(()),
            Self::GetPlayerInfo { pid }
            | Self::GetPlayState { pid }
            | Self::GetNowPlayingMedia { pid }
            | Self::GetPlayMode { pid }
            | Self::PlayNext { pid }
            | Self::PlayPrevious { pid }
            | Self::ClearQueue { pid }
            | Self::GetQuickSelects { pid } => write!(out, "?pid={pid}"),
            Self::SetPlayState { pid, state } => {
                write!(out, "?pid={pid}&state={}", state.name())
            }
            Self::SetPlayMode {
                pid,
                repeat,
                shuffle,
            } => write!(
                out,
                "?pid={pid}&repeat={}&shuffle={}",
                repeat.name(),
                on_off(shuffle)
            ),
            Self::GetQueue { pid, range } => {
                write!(out, "?pid={pid}")?;
                write_range(out, range)
            }
            Self::PlayQueue { pid, qid } => write!(out, "?pid={pid}&qid={qid}"),
            Self::RemoveFromQueue { pid, qids } => {
                write!(out, "?pid={pid}&qid=")?;
                for (index, qid) in qids.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write!(out, "{qid}")?;
                }
                Ok(())
            }
            Self::PlayQuickSelect { pid, id } => write!(out, "?pid={pid}&id={id}"),
            Self::GetFavorites { range } => {
                write!(out, "?sid={}", SourceId::FAVORITES)?;
                write_range(out, range)
            }
            Self::PlayPreset { pid, preset } => write!(out, "?pid={pid}&preset={preset}"),
            Self::PlayInput { pid, source, input } => {
                write!(out, "?pid={pid}")?;
                if let Some(spid) = source {
                    write!(out, "&spid={spid}")?;
                }
                out.push_str("&input=");
                push_encoded(out, input);
                Ok(())
            }
            Self::PlayStream { pid, url } => {
                // The URL must be the last argument.
                write!(out, "?pid={pid}&url=")?;
                push_encoded(out, url);
                Ok(())
            }
        }
    }
}

const fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

fn write_range(out: &mut String, range: Option<(u32, u32)>) -> std::fmt::Result {
    match range {
        Some((first, last)) => write!(out, "&range={first},{last}"),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(command: Command<'_>) -> String {
        let mut out = String::new();
        command.write(&mut out).unwrap();
        out
    }

    #[test]
    fn writes_commands() {
        let pid = PlayerId(-12);
        let cases = [
            (
                Command::RegisterForChangeEvents { enable: true },
                "heos://system/register_for_change_events?enable=on",
            ),
            (Command::HeartBeat, "heos://system/heart_beat"),
            (Command::GetPlayers, "heos://player/get_players"),
            (
                Command::GetPlayerInfo { pid },
                "heos://player/get_player_info?pid=-12",
            ),
            (
                Command::SetPlayState {
                    pid,
                    state: PlayState::Pause,
                },
                "heos://player/set_play_state?pid=-12&state=pause",
            ),
            (
                Command::SetPlayMode {
                    pid,
                    repeat: RepeatMode::OnAll,
                    shuffle: false,
                },
                "heos://player/set_play_mode?pid=-12&repeat=on_all&shuffle=off",
            ),
            (
                Command::GetQueue {
                    pid,
                    range: Some((0, 9)),
                },
                "heos://player/get_queue?pid=-12&range=0,9",
            ),
            (
                Command::GetQueue { pid, range: None },
                "heos://player/get_queue?pid=-12",
            ),
            (
                Command::RemoveFromQueue {
                    pid,
                    qids: &[QueueId(4), QueueId(5)],
                },
                "heos://player/remove_from_queue?pid=-12&qid=4,5",
            ),
            (
                Command::PlayQuickSelect { pid, id: 6 },
                "heos://player/play_quickselect?pid=-12&id=6",
            ),
            (
                Command::GetFavorites { range: None },
                "heos://browse/browse?sid=1028",
            ),
            (
                Command::PlayPreset { pid, preset: 1 },
                "heos://browse/play_preset?pid=-12&preset=1",
            ),
            (
                Command::PlayInput {
                    pid,
                    source: Some(PlayerId(7)),
                    input: "inputs/aux_in_1",
                },
                "heos://browse/play_input?pid=-12&spid=7&input=inputs/aux_in_1",
            ),
            (
                Command::PlayStream {
                    pid,
                    url: "http://host/a.mp3?x=1&y=50%",
                },
                "heos://browse/play_stream?pid=-12&url=http://host/a.mp3?x%3D1%26y%3D50%25",
            ),
        ];
        for (command, expected) in cases {
            assert_eq!(line(command), format!("{expected}\r\n"));
            assert!(expected[7..].starts_with(command.name()), "{expected}");
        }
    }

    #[test]
    fn rejects_bad_values() {
        let pid = PlayerId(1);
        let cases = [
            (
                Command::PlayQuickSelect { pid, id: 0 },
                Error::ValueOutOfRange {
                    what: "quick select",
                },
            ),
            (
                Command::PlayQuickSelect { pid, id: 7 },
                Error::ValueOutOfRange {
                    what: "quick select",
                },
            ),
            (
                Command::PlayPreset { pid, preset: 0 },
                Error::ValueOutOfRange { what: "preset" },
            ),
            (
                Command::GetQueue {
                    pid,
                    range: Some((5, 4)),
                },
                Error::ValueOutOfRange { what: "range" },
            ),
            (
                Command::RemoveFromQueue { pid, qids: &[] },
                Error::ValueOutOfRange { what: "queue ids" },
            ),
            (
                Command::PlayStream { pid, url: "" },
                Error::InvalidCommandText,
            ),
            (
                Command::PlayStream {
                    pid,
                    url: "http://a\r\nheos://system/reboot",
                },
                Error::InvalidCommandText,
            ),
            (
                Command::PlayInput {
                    pid,
                    source: None,
                    input: "inputs/aux\n",
                },
                Error::InvalidCommandText,
            ),
        ];
        for (command, expected) in cases {
            let mut out = String::from("kept");
            assert_eq!(command.write(&mut out), Err(expected.clone()));
            assert_eq!(out, "kept");
            assert!(expected.is_invalid_input());
        }
    }
}
