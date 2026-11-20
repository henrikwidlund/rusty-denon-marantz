//! Sans-IO protocol for the HEOS CLI of Denon and Marantz products.
//!
//! The CLI listens on TCP port [`PORT`]. Commands are text lines, answers and
//! events are JSON lines. This crate builds the commands and parses the
//! lines. It does no I/O.
//!
//! The types return what the device sends. For example, a player can lack its
//! IP address, and an answer can have no payload. The async client in the
//! `denon-marantz-heos` crate decides what to show.
//!
//! # Example
//!
//! ```
//! use denon_marantz_heos_proto::{Command, LineDecoder, PlayState, PlayerId, Response};
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut out = String::new();
//! Command::GetPlayState { pid: PlayerId(1) }.write(&mut out)?;
//! assert_eq!(out, "heos://player/get_play_state?pid=1\r\n");
//! // Write `out` to the device, then give the bytes that come back to a decoder.
//!
//! let received = b"{\"heos\": {\"command\": \"player/get_play_state\", \"result\": \"success\", \"message\": \"pid=1&state=play\"}}\r\n";
//! let mut decoder = LineDecoder::new();
//! let mut states = Vec::new();
//! decoder.decode(received, |line| {
//!     if let Ok(response) = Response::parse(line) {
//!         states.push(response.play_state());
//!     }
//! });
//! assert_eq!(states, [Ok(Some(PlayState::Play))]);
//! # Ok(())
//! # }
//! ```
//!
//! # Values in commands and answers
//!
//! The URL of [`Command::PlayStream`] is sent as it is: the device does not
//! decode it. `&`, `=` and `%` in the name of [`Command::PlayInput`] are
//! encoded. Values in the `message` of answers and events are decoded by
//! [`Message::get`]. Text in the payload is used as the device sends it: the
//! AVC-A10H sends `&` in a song name as it is, not as `%26`.

mod command;
mod controls;
mod de;
mod decoder;
mod error;
mod event;
mod message;
mod response;
mod types;

pub use command::Command;
pub use controls::Controls;
pub use decoder::LineDecoder;
pub use error::Error;
pub use event::Event;
pub use message::{Message, decode};
pub use response::{ErrorCode, Failure, Response, Status};
pub use types::{
    BrowseItem, MediaType, NowPlaying, PlayMode, PlayState, Player, PlayerId, Progress, QueueId,
    QueueItem, QuickSelect, RepeatMode, SourceId,
};

/// TCP port of the HEOS CLI.
pub const PORT: u16 = 1255;
