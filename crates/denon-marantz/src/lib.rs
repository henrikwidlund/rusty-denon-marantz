//! Async client to monitor and control Denon and Marantz AV receivers.

#[cfg(feature = "http")]
mod channels;
mod error;
mod pacer;

#[cfg(feature = "http")]
mod avr;
#[cfg(feature = "http")]
mod http;
#[cfg(feature = "http")]
mod identify;
#[cfg(feature = "telnet")]
mod telnet;

pub use error::Error;
pub use pacer::{Adaptive, Pacer, Pacing};

#[cfg(feature = "http")]
pub use avr::{Avr, AvrBuilder, Info, UpdateMethod};
#[cfg(feature = "telnet")]
pub use denon_marantz_proto::telnet::Command;
#[cfg(feature = "http")]
pub use http::{HttpClient, HttpConfig};
#[cfg(feature = "http")]
pub use identify::{Identified, identify, identify_on};
#[cfg(feature = "telnet")]
pub use telnet::{TelnetConfig, TelnetEvent, TelnetStatus};
