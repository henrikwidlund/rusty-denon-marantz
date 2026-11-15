//! Async client for the HEOS CLI of Denon and Marantz products.
//!
//! It reads what plays, the play state, the position and the repeat and
//! shuffle modes, and it controls the transport. It also reads and plays the
//! queue, HEOS Favorites and quick selects, and plays streams and inputs.
//! Volume and mute are not here: on an AV receiver, HEOS mixes the
//! volume of the zones, so use telnet or HTTP for them.
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
