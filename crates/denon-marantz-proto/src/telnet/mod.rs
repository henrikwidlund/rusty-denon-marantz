//! Telnet protocol on port 23, without IO.
//!
//! The client writes lines made with [`Command::write_line`]. Bytes from the
//! receiver go through a [`LineDecoder`], and each line through
//! [`Message::parse`]. A receiver allows one telnet connection at a time.

mod command;
mod decoder;
mod event;
mod query;

pub use command::Command;
pub use decoder::LineDecoder;
pub use event::{DuplicateFilter, Event, EventZone, Message};
pub use query::{
    DEVICE_POWER_QUERY, MAIN_ZONE_POWER_QUERY, QueryGroup, group_queries, queries, state_queries,
};
