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
//!   `telnet_refresh`, `telnet_events`.
//! - `refresh*` picks the way for you: `refresh`, `refresh_signal` and
//!   `refresh_now_playing`.
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
//! let started = avr.start().await?;
//! println!("{} is {:?}", avr.info().friendly_name, avr.snapshot().zone(Zone::Main).power);
//!
//! let mut changes = avr.changes();
//! loop {
//!     match changes.recv().await {
//!         Ok(change) => println!("{change:?}: {:?}", avr.snapshot().zone(Zone::Main)),
//!         // Some changes were lost. The state is still right, so read it again.
//!         Err(RecvError::Lagged(_)) => println!("{:?}", avr.snapshot().zone(Zone::Main)),
//!         Err(RecvError::Closed) => break,
//!     }
//! }
//! // Optional: wait until everything that the start reads has been read.
//! let report = started.finished().await;
//! # Ok(())
//! # }
//! ```
//!
//! With the default `telnet` feature, `Avr::zone` and the other controls send
//! commands, for example `avr.zone(Zone::Main)?.set_volume_db(-35.0)`.

#[cfg(feature = "http")]
mod channels;
#[cfg(feature = "http")]
mod details;
mod error;
mod pacer;

#[cfg(feature = "telnet")]
mod audyssey;
#[cfg(feature = "http")]
mod avr;
#[cfg(feature = "telnet")]
mod device_control;
#[cfg(feature = "telnet")]
mod dirac;
#[cfg(feature = "discovery")]
mod discovery;
#[cfg(feature = "http")]
mod http;
#[cfg(feature = "http")]
mod identify;
#[cfg(feature = "telnet")]
mod sound_control;
#[cfg(feature = "telnet")]
mod telnet;
#[cfg(feature = "telnet")]
mod tone_control;
#[cfg(feature = "telnet")]
mod tuner_control;
#[cfg(feature = "telnet")]
mod video_control;
#[cfg(feature = "telnet")]
mod zone_control;

pub use denon_marantz_proto as proto;
pub use denon_marantz_proto::{
    Brand, Change, Channel, ChannelLevel, DeviceState, NowPlaying, PlayingKind, ReceiverType,
    Setting, SoundModeEntry, State, ToneLevel, Volume, Zone, ZoneState,
};
pub use error::{Error, ToneBlocker};
pub use pacer::{Adaptive, Pacer, Pacing};

#[cfg(feature = "telnet")]
pub use audyssey::Audyssey;
#[cfg(feature = "http")]
pub use avr::{
    Avr, AvrBuilder, Info, StartOptions, StartPages, StartReport, Started, TelnetStart,
    UpdateMethod,
};
#[cfg(feature = "telnet")]
pub use denon_marantz_proto::RemoteKey;
#[cfg(feature = "telnet")]
pub use denon_marantz_proto::telnet::{Command, QueryGroup};
#[cfg(feature = "http")]
pub use details::{
    GeneralDetails, PageResults, PageStatus, SettingsAvailabilityDetails, SkipReason,
    SourceDetails, SpeakerDetails, VideoMenuDetails,
};
#[cfg(feature = "telnet")]
pub use device_control::{DeviceControl, ResetKey};
#[cfg(feature = "telnet")]
pub use dirac::Dirac;
#[cfg(feature = "discovery")]
pub use discovery::{DiscoveredReceiver, DiscoveryConfig, LocationPolicy, discover};
#[cfg(feature = "http")]
pub use http::{HttpClient, HttpConfig};
#[cfg(feature = "http")]
pub use identify::{Identified, identify, identify_on};
#[cfg(feature = "telnet")]
pub use sound_control::SoundControl;
#[cfg(feature = "telnet")]
pub use telnet::{TelnetConfig, TelnetEvent, TelnetStatus};
#[cfg(feature = "telnet")]
pub use tone_control::ToneControl;
#[cfg(feature = "telnet")]
pub use tuner_control::TunerControl;
#[cfg(feature = "telnet")]
pub use video_control::VideoControl;
#[cfg(feature = "telnet")]
pub use zone_control::ZoneControl;
