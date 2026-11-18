//! Commands to the CLI.

use std::fmt::Write as _;

use crate::error::Error;
use crate::types::{MAX_VOLUME, PlayState, PlayerId, QueueId, RepeatMode, SourceId};

/// Highest quick select slot.
const MAX_QUICK_SELECT: u8 = 6;

/// Highest volume step.
const MAX_VOLUME_STEP: u8 = 10;

/// A command to the CLI.
///
/// Group membership, sign-in and service browsing are not here.
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
    /// Reads the volume.
    GetVolume {
        /// Player.
        pid: PlayerId,
    },
    /// Sets the volume.
    SetVolume {
        /// Player.
        pid: PlayerId,
        /// Level from 0 to 100.
        level: u8,
    },
    /// Turns the volume up.
    VolumeUp {
        /// Player.
        pid: PlayerId,
        /// Step from 1 to 10. `None` uses the step of the device, 5.
        step: Option<u8>,
    },
    /// Turns the volume down.
    VolumeDown {
        /// Player.
        pid: PlayerId,
        /// Step from 1 to 10. `None` uses the step of the device, 5.
        step: Option<u8>,
    },
    /// Reads if the player is muted.
    GetMute {
        /// Player.
        pid: PlayerId,
    },
    /// Mutes or unmutes.
    SetMute {
        /// Player.
        pid: PlayerId,
        /// Mute on.
        mute: bool,
    },
    /// Mutes if not muted, else unmutes.
    ToggleMute {
        /// Player.
        pid: PlayerId,
    },
    /// Reads the volume of a group.
    GetGroupVolume {
        /// Group.
        gid: PlayerId,
    },
    /// Sets the volume of a group.
    SetGroupVolume {
        /// Group.
        gid: PlayerId,
        /// Level from 0 to 100.
        level: u8,
    },
    /// Turns the volume of a group up.
    GroupVolumeUp {
        /// Group.
        gid: PlayerId,
        /// Step from 1 to 10. `None` uses the step of the device, 5.
        step: Option<u8>,
    },
    /// Turns the volume of a group down.
    GroupVolumeDown {
        /// Group.
        gid: PlayerId,
        /// Step from 1 to 10. `None` uses the step of the device, 5.
        step: Option<u8>,
    },
    /// Reads if a group is muted.
    GetGroupMute {
        /// Group.
        gid: PlayerId,
    },
    /// Mutes or unmutes a group.
    SetGroupMute {
        /// Group.
        gid: PlayerId,
        /// Mute on.
        mute: bool,
    },
    /// Mutes a group if not muted, else unmutes it.
    ToggleGroupMute {
        /// Group.
        gid: PlayerId,
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
        /// Input name. It is sent as it is: the device does not decode it.
        input: &'a str,
    },
    /// Plays a stream from a URL.
    PlayStream {
        /// Player.
        pid: PlayerId,
        /// Absolute URL. It is sent as it is: the device does not decode it.
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
            Self::GetVolume { .. } => "player/get_volume",
            Self::SetVolume { .. } => "player/set_volume",
            Self::VolumeUp { .. } => "player/volume_up",
            Self::VolumeDown { .. } => "player/volume_down",
            Self::GetMute { .. } => "player/get_mute",
            Self::SetMute { .. } => "player/set_mute",
            Self::ToggleMute { .. } => "player/toggle_mute",
            Self::GetGroupVolume { .. } => "group/get_volume",
            Self::SetGroupVolume { .. } => "group/set_volume",
            Self::GroupVolumeUp { .. } => "group/volume_up",
            Self::GroupVolumeDown { .. } => "group/volume_down",
            Self::GetGroupMute { .. } => "group/get_mute",
            Self::SetGroupMute { .. } => "group/set_mute",
            Self::ToggleGroupMute { .. } => "group/toggle_mute",
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
            Self::SetVolume { level, .. } | Self::SetGroupVolume { level, .. }
                if level > MAX_VOLUME =>
            {
                Err(Error::ValueOutOfRange { what: "volume" })
            }
            Self::VolumeUp {
                step: Some(step), ..
            }
            | Self::VolumeDown {
                step: Some(step), ..
            }
            | Self::GroupVolumeUp {
                step: Some(step), ..
            }
            | Self::GroupVolumeDown {
                step: Some(step), ..
            } if !(1..=MAX_VOLUME_STEP).contains(&step) => Err(Error::ValueOutOfRange {
                what: "volume step",
            }),
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
            | Self::GetQuickSelects { pid }
            | Self::GetVolume { pid }
            | Self::GetMute { pid }
            | Self::ToggleMute { pid } => write!(out, "?pid={pid}"),
            Self::SetVolume { pid, level } => write!(out, "?pid={pid}&level={level}"),
            Self::VolumeUp { pid, step } | Self::VolumeDown { pid, step } => {
                write!(out, "?pid={pid}")?;
                write_step(out, step)
            }
            Self::SetMute { pid, mute } => write!(out, "?pid={pid}&state={}", on_off(mute)),
            Self::GetGroupVolume { gid }
            | Self::GetGroupMute { gid }
            | Self::ToggleGroupMute { gid } => write!(out, "?gid={gid}"),
            Self::SetGroupVolume { gid, level } => write!(out, "?gid={gid}&level={level}"),
            Self::GroupVolumeUp { gid, step } | Self::GroupVolumeDown { gid, step } => {
                write!(out, "?gid={gid}")?;
                write_step(out, step)
            }
            Self::SetGroupMute { gid, mute } => write!(out, "?gid={gid}&state={}", on_off(mute)),
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
                write!(out, "&input={input}")
            }
            Self::PlayStream { pid, url } => {
                // The URL must be the last argument, so that `&` and `=` in it
                // do not end it.
                write!(out, "?pid={pid}&url={url}")
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

fn write_step(out: &mut String, step: Option<u8>) -> std::fmt::Result {
    step.map_or(Ok(()), |step| write!(out, "&step={step}"))
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
                "heos://browse/play_stream?pid=-12&url=http://host/a.mp3?x=1&y=50%",
            ),
        ];
        for (command, expected) in cases {
            assert_eq!(line(command), format!("{expected}\r\n"));
            assert!(expected[7..].starts_with(command.name()), "{expected}");
        }
    }

    #[test]
    fn writes_volume_and_mute_commands() {
        let pid = PlayerId(-12);
        let cases = [
            (
                Command::GetVolume { pid },
                "heos://player/get_volume?pid=-12",
            ),
            (
                Command::SetVolume { pid, level: 100 },
                "heos://player/set_volume?pid=-12&level=100",
            ),
            (
                Command::SetVolume { pid, level: 0 },
                "heos://player/set_volume?pid=-12&level=0",
            ),
            (
                Command::VolumeUp {
                    pid,
                    step: Some(10),
                },
                "heos://player/volume_up?pid=-12&step=10",
            ),
            (
                Command::VolumeUp { pid, step: None },
                "heos://player/volume_up?pid=-12",
            ),
            (
                Command::VolumeDown { pid, step: Some(1) },
                "heos://player/volume_down?pid=-12&step=1",
            ),
            (
                Command::VolumeDown { pid, step: None },
                "heos://player/volume_down?pid=-12",
            ),
            (Command::GetMute { pid }, "heos://player/get_mute?pid=-12"),
            (
                Command::SetMute { pid, mute: true },
                "heos://player/set_mute?pid=-12&state=on",
            ),
            (
                Command::SetMute { pid, mute: false },
                "heos://player/set_mute?pid=-12&state=off",
            ),
            (
                Command::ToggleMute { pid },
                "heos://player/toggle_mute?pid=-12",
            ),
            (
                Command::GetGroupVolume { gid: pid },
                "heos://group/get_volume?gid=-12",
            ),
            (
                Command::SetGroupVolume {
                    gid: pid,
                    level: 30,
                },
                "heos://group/set_volume?gid=-12&level=30",
            ),
            (
                Command::GroupVolumeUp {
                    gid: pid,
                    step: Some(5),
                },
                "heos://group/volume_up?gid=-12&step=5",
            ),
            (
                Command::GroupVolumeDown {
                    gid: pid,
                    step: None,
                },
                "heos://group/volume_down?gid=-12",
            ),
            (
                Command::GroupVolumeDown {
                    gid: pid,
                    step: Some(2),
                },
                "heos://group/volume_down?gid=-12&step=2",
            ),
            (
                Command::GetGroupMute { gid: pid },
                "heos://group/get_mute?gid=-12",
            ),
            (
                Command::SetGroupMute {
                    gid: pid,
                    mute: true,
                },
                "heos://group/set_mute?gid=-12&state=on",
            ),
            (
                Command::ToggleGroupMute { gid: pid },
                "heos://group/toggle_mute?gid=-12",
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
                Command::SetVolume { pid, level: 101 },
                Error::ValueOutOfRange { what: "volume" },
            ),
            (
                Command::SetGroupVolume {
                    gid: pid,
                    level: 101,
                },
                Error::ValueOutOfRange { what: "volume" },
            ),
            (
                Command::VolumeUp { pid, step: Some(0) },
                Error::ValueOutOfRange {
                    what: "volume step",
                },
            ),
            (
                Command::VolumeDown {
                    pid,
                    step: Some(11),
                },
                Error::ValueOutOfRange {
                    what: "volume step",
                },
            ),
            (
                Command::GroupVolumeUp {
                    gid: pid,
                    step: Some(11),
                },
                Error::ValueOutOfRange {
                    what: "volume step",
                },
            ),
            (
                Command::GroupVolumeDown {
                    gid: pid,
                    step: Some(0),
                },
                Error::ValueOutOfRange {
                    what: "volume step",
                },
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

    proptest::proptest! {
        #[test]
        fn text_values_give_one_line_or_an_error(text in proptest::prelude::any::<String>()) {
            let pid = PlayerId(1);
            let commands = [
                Command::PlayStream { pid, url: &text },
                Command::PlayInput { pid, source: None, input: &text },
            ];
            for command in commands {
                let mut out = String::new();
                if command.write(&mut out).is_ok() {
                    let body = out.strip_suffix("\r\n").unwrap_or_default();
                    proptest::prop_assert!(!body.contains(['\r', '\n']), "{out:?}");
                    proptest::prop_assert!(body.ends_with(&*text), "{out:?}");
                } else {
                    proptest::prop_assert!(out.is_empty());
                }
            }
        }
    }
}
