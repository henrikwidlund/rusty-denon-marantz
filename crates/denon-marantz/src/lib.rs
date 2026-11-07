//! Async client to monitor and control Denon and Marantz AV receivers.

#[cfg(feature = "http")]
mod channels;
mod error;
mod pacer;

#[cfg(feature = "telnet")]
mod audyssey;
#[cfg(feature = "http")]
mod avr;
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
mod zone_control;

pub use error::{Error, ToneBlocker};
pub use pacer::{Adaptive, Pacer, Pacing};

#[cfg(feature = "telnet")]
pub use audyssey::Audyssey;
#[cfg(feature = "http")]
pub use avr::{Avr, AvrBuilder, Info, UpdateMethod};
#[cfg(feature = "telnet")]
pub use denon_marantz_proto::RemoteKey;
#[cfg(feature = "telnet")]
pub use denon_marantz_proto::telnet::Command;
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
pub use zone_control::ZoneControl;
