//! Async client to monitor and control Denon and Marantz AV receivers.
//!
//! The types of the protocol are in the `denon-marantz-proto` crate. This
//! crate has them too: the types that most programs need are at the top, for
//! example [`Zone`] and [`Volume`], and all others are in [`proto`]. This
//! includes the values that the setters take, for example `DimmerMode`,
//! `DiracFilter` and `TunerBand`.
//!
//! # Naming
//!
//! The names of the calls say which way they use:
//! - `update_*` reads over HTTP: `update` reads the zones, the others read
//!   pages of the web interface on port 11080 or the status pages.
//! - `telnet_*` uses the telnet connection: `telnet_connect`, `telnet_send`,
//!   `telnet_refresh`, `telnet_ask`, `telnet_events`.
//! - `refresh*` picks the way for you: `refresh`, `refresh_signal` and
//!   `refresh_now_playing`.
//! - `send` and `http_send` send a command: `send` uses telnet while it is open
//!   and HTTP otherwise.
//! - The setters are in the controls: `Avr::zone`, `sound`, `device`, `tone`,
//!   `tuner`, `video`, `audyssey` and `dirac`. `select_` chooses an item from
//!   a list, `set_` sets a value, `store_` saves the current setting in an
//!   item, `_next` and `_previous` step in a list and `_up` and `_down` step
//!   a number.
//!
//! `HttpClient`, `Pacer` and `identify` are for programs that need more
//! control. Most programs only use `Avr`.
//!
//! # Example
//!
//! `Avr::start` reads the zones and returns. Telnet and the web interface
//! fill in the rest in the background, and `Avr::changes` reports each
//! change of the state.
//!
//! ```no_run
//! use std::sync::Arc;
//! use denon_marantz::{Avr, Zone};
//! use tokio::sync::broadcast::error::RecvError;
//!
//! # async fn run() -> Result<(), denon_marantz::Error> {
//! let avr = Arc::new(Avr::builder("192.168.1.20").connect().await?);
//! // Subscribe before the start, so that no change is missed.
//! let mut changes = avr.changes();
//! let mut started = avr.start().await?;
//! println!("{} is {:?}", avr.info().friendly_name, avr.snapshot().zone(Zone::Main).power);
//!
//! // Optional: wait until the start has read everything and the receiver has
//! // answered the telnet queries (`basic_synced` does not wait for telnet).
//! let report = started.all_synced().await;
//!
//! loop {
//!     match changes.recv().await {
//!         Ok(change) => println!("{change:?}: {:?}", avr.snapshot().zone(Zone::Main)),
//!         // Some changes were lost. The state is still right, so read it again.
//!         Err(RecvError::Lagged(_)) => println!("{:?}", avr.snapshot().zone(Zone::Main)),
//!         Err(RecvError::Closed) => break,
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! `Avr::zone` and the other controls send commands, for example
//! `avr.zone(Zone::Main)?.set_volume_db(-35.0)`. They use telnet while it is
//! open. A program that does not open telnet, for example with
//! `StartOptions::new().telnet(false)`, sends over HTTP: the receiver does not
//! send events then, so the state changes when it is read. After a command over
//! HTTP, the values that the command changed are read, see
//! `AvrBuilder::follow_up_read`.

#[cfg(feature = "http")]
mod channels;
#[cfg(feature = "http")]
mod command;
#[cfg(feature = "http")]
mod details;
mod error;
mod pacer;

#[cfg(feature = "http")]
mod audyssey;
#[cfg(feature = "http")]
mod avr;
#[cfg(feature = "http")]
mod device_control;
#[cfg(feature = "http")]
mod dirac;
#[cfg(feature = "discovery")]
mod discovery;
#[cfg(feature = "heos")]
mod heos_link;
#[cfg(feature = "http")]
mod host;
#[cfg(feature = "http")]
mod http;
#[cfg(feature = "http")]
mod identify;
#[cfg(feature = "http")]
mod priority;
#[cfg(feature = "http")]
mod sound_control;
#[cfg(feature = "http")]
mod status;
#[cfg(feature = "telnet")]
mod telnet;
#[cfg(feature = "http")]
mod tone_control;
#[cfg(feature = "http")]
mod tuner_control;
#[cfg(feature = "http")]
mod video_control;
#[cfg(feature = "http")]
mod zone_control;

#[cfg(feature = "heos")]
pub use denon_marantz_heos as heos;
pub use denon_marantz_proto as proto;
pub use denon_marantz_proto::{
    Brand, Change, Channel, ChannelLayout, ChannelLevel, DeviceFeature, DeviceFeatures,
    DeviceRanges, DeviceState, InOut, ListedQueries, NowPlaying, PlayingKind, ReceiverType,
    Setting, SignalState, SoundModeEntry, State, ToneLevel, TunerBands, TunerInfo, ValueRange,
    VideoColor, Volume, Zone, ZoneRanges, ZoneState,
};
pub use error::{Error, ToneBlocker};
#[cfg(feature = "heos")]
pub use heos_link::{HeosFailure, HeosPlayer, HeosPlayerStatus};
pub use pacer::{Adaptive, Pacer, Pacing};

#[cfg(feature = "http")]
pub use audyssey::Audyssey;
#[cfg(feature = "http")]
pub use avr::{
    Avr, AvrBuilder, FollowUpRead, HeosStart, Info, InfoCheck, StartOptions, StartPages,
    StartReport, Started, TelnetStart, TelnetSync, UpdateMethod,
};
#[cfg(feature = "http")]
pub use denon_marantz_proto::RemoteKey;
#[cfg(feature = "http")]
pub use denon_marantz_proto::telnet::Command;
#[cfg(feature = "telnet")]
pub use denon_marantz_proto::telnet::{Query, QueryGroup};
#[cfg(feature = "http")]
pub use details::{
    GeneralDetails, PageResults, PageStatus, SettingsAvailabilityDetails, SkipReason,
    SourceDetails, SpeakerDetails, VideoMenuDetails,
};
#[cfg(feature = "http")]
pub use device_control::{DeviceControl, ResetKey};
#[cfg(feature = "http")]
pub use dirac::Dirac;
#[cfg(feature = "discovery")]
pub use discovery::{DiscoveredReceiver, DiscoveryConfig, LocationPolicy, discover};
#[cfg(feature = "mdns")]
pub use discovery::{discover_all, discover_mdns};
#[cfg(feature = "http")]
pub use http::{HttpClient, HttpConfig};
#[cfg(feature = "http")]
pub use identify::{Identified, TypeProof, identify, identify_on};
#[cfg(feature = "http")]
pub use sound_control::SoundControl;
#[cfg(feature = "http")]
pub use status::{Blocker, Feature, SettingStatus};
#[cfg(feature = "telnet")]
pub use telnet::{Ask, TelnetConfig, TelnetEvent, TelnetStatus};
#[cfg(feature = "http")]
pub use tone_control::ToneControl;
#[cfg(feature = "http")]
pub use tuner_control::TunerControl;
#[cfg(feature = "http")]
pub use video_control::VideoControl;
#[cfg(feature = "http")]
pub use zone_control::ZoneControl;
