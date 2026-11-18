//! A JSON line from the CLI: an answer to a command or an event.

use std::borrow::Cow;

use serde::Deserialize;
use serde::de::IgnoredAny;
use serde_json::value::RawValue;

use crate::de;
use crate::error::Error;
use crate::event::Event;
use crate::message::Message;
use crate::types::{MAX_VOLUME, NowPlaying, PlayMode, PlayState, RepeatMode};

/// Prefix of the command name of an event.
const EVENT_PREFIX: &str = "event/";

/// First part of the message while the CLI still works on a command.
const UNDER_PROCESS: &str = "command under process";

/// A parsed line from the CLI.
///
/// Text borrows from the line where it can.
#[derive(Debug, Clone)]
pub struct Response<'a> {
    command: Cow<'a, str>,
    result: Option<Cow<'a, str>>,
    message: Cow<'a, str>,
    payload: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct Line<'a> {
    #[serde(borrow)]
    heos: Header<'a>,
    #[serde(default, borrow)]
    payload: Option<&'a RawValue>,
}

#[derive(Deserialize)]
struct Header<'a> {
    #[serde(borrow, deserialize_with = "de::text")]
    command: Cow<'a, str>,
    #[serde(default, borrow, deserialize_with = "de::option_text")]
    result: Option<Cow<'a, str>>,
    #[serde(default, borrow, deserialize_with = "de::text")]
    message: Cow<'a, str>,
}

#[derive(Deserialize)]
struct TypeOnly {
    #[serde(rename = "type")]
    media_type: Option<IgnoredAny>,
}

impl<'a> Response<'a> {
    /// Parses one line.
    pub fn parse(line: &'a str) -> Result<Self, Error> {
        let Line { heos, payload } = serde_json::from_str(line)?;
        Ok(Self {
            command: heos.command,
            result: heos.result,
            message: heos.message,
            payload,
        })
    }

    /// Command name, for example `player/get_play_state`.
    #[must_use]
    pub fn command(&self) -> &str {
        self.command.trim()
    }

    /// The `message` fields.
    #[must_use]
    pub fn message(&self) -> Message<'_> {
        Message::new(&self.message)
    }

    /// Returns true if the line is an event.
    #[must_use]
    pub fn is_event(&self) -> bool {
        self.command().starts_with(EVENT_PREFIX)
    }

    /// The event, or `None` if the line is an answer.
    pub fn event(&self) -> Result<Option<Event<'_>>, Error> {
        self.command()
            .strip_prefix(EVENT_PREFIX)
            .map(|name| Event::parse(name, self.message()))
            .transpose()
    }

    /// Result of an answer. Call it only for answers: an event has no
    /// result and gives a failure.
    ///
    /// A result other than `success` is a failure.
    #[must_use]
    pub fn status(&self) -> Status<'_> {
        let message = self.message();
        if message.has(UNDER_PROCESS) {
            Status::UnderProcess
        } else if self.result.as_deref().map(str::trim) == Some("success") {
            Status::Success
        } else {
            Status::Fail(Failure::from_message(message))
        }
    }

    /// The payload as JSON text, if there is one.
    #[must_use]
    pub fn raw_payload(&self) -> Option<&'a str> {
        self.payload.map(RawValue::get)
    }

    /// Parses the payload.
    pub fn payload<T: Deserialize<'a>>(&self) -> Result<T, Error> {
        let payload = self.raw_payload().ok_or(Error::NoPayload)?;
        Ok(serde_json::from_str(payload)?)
    }

    /// Total number of items from the `count` field of `get_queue` and
    /// `browse`, or `None` if it is missing or not valid. For `browse`, 0 can
    /// mean that the number is not known.
    #[must_use]
    pub fn count(&self) -> Option<u32> {
        self.message().parse("count").ok()
    }

    /// Play state from the message of `get_play_state`. `None` means that
    /// the device sent `unknown`.
    pub fn play_state(&self) -> Result<Option<PlayState>, Error> {
        self.message().play_state()
    }

    /// Repeat and shuffle modes from the message of `get_play_mode` and
    /// `set_play_mode`.
    pub fn play_mode(&self) -> Result<PlayMode, Error> {
        let message = self.message();
        Ok(PlayMode {
            repeat: message.parse_with("repeat", RepeatMode::from_name)?,
            shuffle: message.on_off("shuffle")?,
        })
    }

    /// Volume level from 0 to 100, from the message of `get_volume` and
    /// `set_volume` of players and groups.
    pub fn volume(&self) -> Result<u8, Error> {
        self.message().parse_with("level", |value| {
            value
                .trim()
                .parse()
                .ok()
                .filter(|level| *level <= MAX_VOLUME)
        })
    }

    /// Mute state from the message of `get_mute` and `set_mute` of players
    /// and groups.
    pub fn mute(&self) -> Result<bool, Error> {
        self.message().on_off("state")
    }

    /// Parses the payload of `get_now_playing_media`.
    ///
    /// Returns `None` if nothing plays: the payload is `{}` or missing.
    pub fn now_playing(&self) -> Result<Option<NowPlaying<'a>>, Error> {
        let Some(payload) = self.raw_payload() else {
            return Ok(None);
        };
        let TypeOnly { media_type } = serde_json::from_str(payload)?;
        if media_type.is_none() {
            return Ok(None);
        }
        Ok(Some(serde_json::from_str(payload)?))
    }
}

/// Result of an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status<'a> {
    /// The command was done.
    Success,
    /// The CLI still works on the command. The real answer comes later.
    UnderProcess,
    /// The command failed.
    Fail(Failure<'a>),
}

/// Details of a failed command, from `eid`, `text` and `syserrno`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure<'a> {
    /// Error code, if the message has a valid `eid`.
    pub code: Option<ErrorCode>,
    /// Error text. It can be empty.
    pub text: Cow<'a, str>,
    /// System error number, for [`ErrorCode::SystemError`].
    pub system_code: Option<i32>,
}

impl<'a> Failure<'a> {
    fn from_message(message: Message<'a>) -> Self {
        Self {
            code: message
                .get("eid")
                .and_then(|eid| eid.trim().parse().ok())
                .map(ErrorCode::from_id),
            text: message.get("text").unwrap_or_default(),
            system_code: message
                .get("syserrno")
                .and_then(|code| code.trim().parse().ok()),
        }
    }

    /// Copies the borrowed text.
    #[must_use]
    pub fn into_owned(self) -> Failure<'static> {
        Failure {
            code: self.code,
            text: crate::types::owned(self.text),
            system_code: self.system_code,
        }
    }
}

macro_rules! error_codes {
    ($($(#[$meta:meta])* $variant:ident = $id:literal,)*) => {
        /// Error code (`eid`) of a failed command.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum ErrorCode {
            $($(#[$meta])* $variant,)*
            /// A code that is not in the specification.
            Other(i32),
        }

        impl ErrorCode {
            /// Code for an `eid` value.
            #[must_use]
            pub const fn from_id(id: i32) -> Self {
                match id {
                    $($id => Self::$variant,)*
                    other => Self::Other(other),
                }
            }

            /// The `eid` value.
            #[must_use]
            pub const fn id(self) -> i32 {
                match self {
                    $(Self::$variant => $id,)*
                    Self::Other(id) => id,
                }
            }
        }
    };
}

error_codes! {
    /// The command is not known.
    UnrecognizedCommand = 1,
    /// An id is not valid.
    InvalidId = 2,
    /// Wrong number of arguments.
    WrongArguments = 3,
    /// The data is not available.
    DataNotAvailable = 4,
    /// The resource is not available now.
    ResourceNotAvailable = 5,
    /// Invalid credentials.
    InvalidCredentials = 6,
    /// The command could not be done.
    CommandNotExecuted = 7,
    /// No user is signed in.
    NotSignedIn = 8,
    /// A parameter is out of range.
    OutOfRange = 9,
    /// The user was not found.
    UserNotFound = 10,
    /// Internal error.
    InternalError = 11,
    /// System error, see [`Failure::system_code`].
    SystemError = 12,
    /// The CLI still processes the previous command.
    ProcessingPreviousCommand = 13,
    /// The media cannot be played.
    CannotPlay = 14,
    /// The option is not supported.
    OptionNotSupported = 15,
    /// Too many commands wait in the queue.
    TooManyCommands = 16,
    /// The skip limit was reached.
    SkipLimit = 17,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_answer() {
        let response = Response::parse(
            r#"{"heos": {"command": "player/get_play_state", "result": "success", "message": "pid=5&state=play"}}"#,
        )
        .unwrap();
        assert_eq!(response.command(), "player/get_play_state");
        assert_eq!(response.status(), Status::Success);
        assert_eq!(response.message().get("state").as_deref(), Some("play"));
        assert!(!response.is_event());
        assert_eq!(response.event(), Ok(None));
        assert_eq!(response.payload::<()>(), Err(Error::NoPayload));
    }

    #[test]
    fn command_name_with_spaces_is_trimmed() {
        let response = Response::parse(
            r#"{"heos":{"command":" player/play_next ","result":" success ","message":"pid=1"}}"#,
        )
        .unwrap();
        assert_eq!(response.command(), "player/play_next");
        assert_eq!(response.status(), Status::Success);
    }

    #[test]
    fn failure_with_error_code() {
        let response = Response::parse(
            r#"{"heos":{"command":"player/get_play_state","result":"fail","message":"eid=2&text=ID Not Valid&pid=5"}}"#,
        )
        .unwrap();
        assert_eq!(
            response.status(),
            Status::Fail(Failure {
                code: Some(ErrorCode::InvalidId),
                text: "ID Not Valid".into(),
                system_code: None,
            })
        );
    }

    #[test]
    fn failure_with_system_error() {
        let response = Response::parse(
            r#"{"heos":{"command":"browse/play_preset","result":"fail","message":"eid=12&text=System error&syserrno=-9"}}"#,
        )
        .unwrap();
        let Status::Fail(failure) = response.status() else {
            panic!("not a failure");
        };
        assert_eq!(failure.code, Some(ErrorCode::SystemError));
        assert_eq!(failure.system_code, Some(-9));
        assert_eq!(failure.into_owned().text, "System error");
    }

    #[test]
    fn failure_without_details() {
        let response =
            Response::parse(r#"{"heos":{"command":"x/y","result":"fail","message":""}}"#).unwrap();
        assert_eq!(
            response.status(),
            Status::Fail(Failure {
                code: None,
                text: "".into(),
                system_code: None,
            })
        );
        let missing = Response::parse(r#"{"heos":{"command":"x/y"}}"#).unwrap();
        assert!(matches!(missing.status(), Status::Fail(_)));
    }

    #[test]
    fn unknown_error_code() {
        assert_eq!(ErrorCode::from_id(99), ErrorCode::Other(99));
        assert_eq!(ErrorCode::Other(99).id(), 99);
        assert_eq!(ErrorCode::from_id(17).id(), 17);
    }

    #[test]
    fn under_process() {
        let response = Response::parse(
            r#"{"heos":{"command":"browse/browse","result":"success","message":"command under process&sid=1028"}}"#,
        )
        .unwrap();
        assert_eq!(response.status(), Status::UnderProcess);
    }

    #[test]
    fn play_state() {
        let parse = |state: &str| {
            let line = format!(
                r#"{{"heos":{{"command":"player/get_play_state","result":"success","message":"pid=1&state={state}"}}}}"#
            );
            Response::parse(&line).unwrap().play_state()
        };
        assert_eq!(parse("stop"), Ok(Some(PlayState::Stop)));
        assert_eq!(parse("unknown"), Ok(None));
        assert_eq!(parse("rewind"), Err(Error::InvalidValue { field: "state" }));
    }

    #[test]
    fn play_mode_and_count() {
        let response = Response::parse(
            r#"{"heos":{"command":"player/set_play_mode","result":"success","message":"pid=1&repeat=on_one&shuffle=on"}}"#,
        )
        .unwrap();
        assert_eq!(
            response.play_mode(),
            Ok(PlayMode {
                repeat: RepeatMode::OnOne,
                shuffle: true,
            })
        );
        assert_eq!(response.count(), None);
        let bad = Response::parse(
            r#"{"heos":{"command":"player/get_play_mode","result":"success","message":"pid=1&repeat=all&shuffle=off&count=x"}}"#,
        )
        .unwrap();
        assert_eq!(
            bad.play_mode(),
            Err(Error::InvalidValue { field: "repeat" })
        );
        assert_eq!(bad.count(), None);
        let page = Response::parse(
            r#"{"heos":{"command":"browse/browse","result":"success","message":"sid=1028&returned=2&count= 7 "}}"#,
        )
        .unwrap();
        assert_eq!(page.count(), Some(7));
        assert_eq!(
            page.play_mode(),
            Err(Error::MissingField { field: "repeat" })
        );
    }

    #[test]
    fn volume_and_mute() {
        fn parse(line: &str) -> Response<'_> {
            Response::parse(line).unwrap()
        }
        let player = parse(
            r#"{"heos":{"command":"player/get_volume","result":"success","message":"pid=1&level=37"}}"#,
        );
        assert_eq!(player.volume(), Ok(37));
        let group = parse(
            r#"{"heos":{"command":"group/set_volume","result":"success","message":"gid=-3&level= 100 "}}"#,
        );
        assert_eq!(group.volume(), Ok(100));
        let high = parse(
            r#"{"heos":{"command":"player/get_volume","result":"success","message":"pid=1&level=101"}}"#,
        );
        assert_eq!(high.volume(), Err(Error::InvalidValue { field: "level" }));
        let muted = parse(
            r#"{"heos":{"command":"player/get_mute","result":"success","message":"pid=1&state=on"}}"#,
        );
        assert_eq!(muted.mute(), Ok(true));
        assert_eq!(muted.volume(), Err(Error::MissingField { field: "level" }));
        let unmuted = parse(
            r#"{"heos":{"command":"group/set_mute","result":"success","message":"gid=-3&state=off"}}"#,
        );
        assert_eq!(unmuted.mute(), Ok(false));
        let bad = parse(
            r#"{"heos":{"command":"player/get_mute","result":"success","message":"pid=1&state=play"}}"#,
        );
        assert_eq!(bad.mute(), Err(Error::InvalidValue { field: "state" }));
    }

    #[test]
    fn empty_now_playing_is_none() {
        for line in [
            r#"{"heos":{"command":"player/get_now_playing_media","result":"success","message":"pid=1"},"payload":{}}"#,
            r#"{"heos":{"command":"player/get_now_playing_media","result":"success","message":"pid=1"},"payload":{"options":[]}}"#,
            r#"{"heos":{"command":"player/get_now_playing_media","result":"success","message":"pid=1"}}"#,
        ] {
            assert_eq!(
                Response::parse(line).unwrap().now_playing(),
                Ok(None),
                "{line}"
            );
        }
    }

    #[test]
    fn text_with_escapes_is_copied() {
        let response = Response::parse(
            r#"{"heos":{"command":"event/player_playback_error","message":"pid=1&error=Bad \"stream\""}}"#,
        )
        .unwrap();
        let Some(Event::PlaybackError { error, .. }) = response.event().unwrap() else {
            panic!("not a playback error");
        };
        assert_eq!(error, "Bad \"stream\"");
    }

    #[test]
    fn rejects_lines_that_are_not_responses() {
        for line in ["", "{}", "[]", r#"{"heos":{}}"#, r#"{"heos":{"command":1"#] {
            assert!(
                matches!(Response::parse(line), Err(Error::InvalidJson { .. })),
                "{line}"
            );
        }
    }
}
