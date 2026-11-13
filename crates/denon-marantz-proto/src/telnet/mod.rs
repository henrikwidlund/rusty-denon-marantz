//! Telnet protocol on port 23, without IO.
//!
//! The client writes lines made with [`Command::write_line`]. Bytes from the
//! receiver go through a [`LineDecoder`], and each line through
//! [`Message::parse`].

mod command;
mod decoder;
mod event;
mod query;

pub use command::Command;
pub use decoder::LineDecoder;
pub use event::{DuplicateFilter, Event, EventZone, Message};
pub use query::{
    DEVICE_POWER_QUERY, MAIN_ZONE_POWER_QUERY, Query, QueryGroup, SignalPart,
    after_appcommand_queries, group_queries, queries, signal_queries, state_queries,
};
