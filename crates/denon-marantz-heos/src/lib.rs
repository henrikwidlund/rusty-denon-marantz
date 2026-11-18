//! Async client for the HEOS CLI of Denon and Marantz products.
//!
//! It reads what plays, the play state, the position and the repeat and
//! shuffle modes, and it controls the transport. It also reads and plays the
//! queue, HEOS Favorites and quick selects, and plays streams and inputs.
//! The client does not keep or control the volume and mute: on the AVC-A10H,
//! the HEOS volume is shared by the zones, so use telnet or HTTP for them. For
//! other HEOS devices, the protocol crate has the volume and mute commands of
//! players and groups, and [`Heos::send`] sends them.
//!
//! The protocol is in the `denon-marantz-heos-proto` crate, also as
//! [`proto`].
//!
//! # Example
//!
//! ```no_run
//! use denon_marantz_heos::Heos;
//! use tokio::sync::broadcast::error::RecvError;
//!
//! # async fn run() -> Result<(), denon_marantz_heos::Error> {
//! let heos = Heos::connect("192.168.1.20").await?;
//! let mut changes = heos.changes();
//! let pid = heos
//!     .player_by_ip("192.168.1.20".parse().unwrap())
//!     .expect("the receiver is a player");
//!
//! loop {
//!     match changes.recv().await {
//!         Ok(_) | Err(RecvError::Lagged(_)) => {
//!             if let Some(state) = heos.player(pid) {
//!                 // A stopped player keeps its last item. `current_media` hides it.
//!                 println!("{:?}: {:?}", state.play_state, state.current_media());
//!             }
//!         }
//!         Err(RecvError::Closed) => break,
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # What the device does
//!
//! - The AVC-A10H has one player for all its zones.
//! - A stopped player keeps its last item, also after the receiver changed to
//!   a source that is not HEOS. [`PlayerState::current_media`] hides it.
//! - The device can report the play state as not known. The state is then
//!   `None`, and the item can be old.
//! - Pausing a station that cannot pause stops it. [`PlayerState::controls`]
//!   says which controls work for the item.
//! - The device sends no event when the HEOS Favorites change. Read them
//!   again with [`Heos::favorites`] when they are needed.
//! - `AirPlay` shows as an item from local music, with no mark that it is
//!   `AirPlay`. In the first seconds after it starts, the item has no text
//!   and the position can jump.
//! - [`Heos::play_stream`] sends the URL as it is, and the device loads it.
//!   The device must be able to reach the URL.
//! - Reading the HEOS Favorites can take longer than other commands. The
//!   device first answers that it works on the command
//!   ([`HeosConfig::slow_command_limit`]).
//! - Commands are sent one at a time. If the future of a command is dropped
//!   before the command is sent, it is not sent.

mod client;
mod error;
mod state;
mod worker;

pub use client::{Answer, Heos, HeosConfig, Page};
pub use denon_marantz_heos_proto as proto;
pub use denon_marantz_heos_proto::{
    BrowseItem, Command, Controls, Event, MediaType, NowPlaying, PlayMode, PlayState, Player,
    PlayerId, Progress, QueueId, QueueItem, QuickSelect, RepeatMode, SourceId,
};
pub use error::Error;
pub use state::{Change, HeosStatus, PlayerState};
