//! Async client to monitor and control Denon and Marantz AV receivers.

mod error;
mod pacer;

#[cfg(feature = "http")]
mod avr;
#[cfg(feature = "http")]
mod http;
#[cfg(feature = "http")]
mod identify;

pub use error::Error;
pub use pacer::{Adaptive, Pacer, Pacing};

#[cfg(feature = "http")]
pub use avr::{Avr, AvrBuilder, Info, UpdateMethod};
#[cfg(feature = "http")]
pub use http::{HttpClient, HttpConfig};
#[cfg(feature = "http")]
pub use identify::{Identified, identify, identify_on};
