//! Sans-IO protocol for the HEOS CLI of Denon and Marantz products.
//!
//! The CLI listens on TCP port [`PORT`]. Commands are text lines, answers and
//! events are JSON lines. This crate builds the commands and parses the
//! lines. It does no I/O.

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
