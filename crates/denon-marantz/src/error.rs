//! Errors of the client.

use std::time::Duration;

use denon_marantz_proto::Zone;

type Source = Box<dyn std::error::Error + Send + Sync>;

/// Error of the client.
///
/// Errors from requests say which request failed. Use the `is_*` methods to
/// react without matching each variant.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The receiver did not answer in time.
    #[error("{request}: no answer within {limit:?}")]
    Timeout {
        /// The request, for example `GET http://10.0.0.5:80/goform/Deviceinfo.xml`.
        request: String,
        /// The time that was allowed.
        limit: Duration,
    },
    /// The connection failed.
    #[error("{request}: network error: {source}")]
    Network {
        /// The request.
        request: String,
        /// Cause.
        #[source]
        source: Source,
    },
    /// The receiver answered with an HTTP status that is not a success.
    #[error("{request}: the receiver answered with HTTP status {status}")]
    Status {
        /// The request.
        request: String,
        /// HTTP status code.
        status: u16,
    },
    /// The receiver refused the request with HTTP status 403.
    #[error("{request}: the receiver refused the request (HTTP 403)")]
    Forbidden {
        /// The request.
        request: String,
    },
    /// The answer is not valid HTTP.
    #[error("{request}: invalid answer: {source}")]
    InvalidResponse {
        /// The request.
        request: String,
        /// Cause.
        #[source]
        source: Source,
    },
    /// The answer is not what the protocol allows, for example HTML or bad XML.
    #[error("{request}: {source}")]
    Protocol {
        /// The request.
        request: String,
        /// Cause.
        #[source]
        source: denon_marantz_proto::Error,
    },
    /// The answer is larger than the limit.
    #[error("{request}: the answer is larger than {limit} bytes")]
    ResponseTooLarge {
        /// The request.
        request: String,
        /// The limit in bytes.
        limit: usize,
    },
    /// The telnet connection is not open, so nothing was sent.
    #[error("the telnet connection is not open")]
    NotConnected,
    /// A command cannot be sent, for example because its text has a control
    /// character.
    #[error("invalid command: {0}")]
    InvalidCommand(#[source] denon_marantz_proto::Error),
    /// A request is not valid: the host or the path.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
    /// The receiver does not have the zone.
    #[error("the receiver has no {}", .0.name())]
    NoSuchZone(Zone),
    /// The zone has no sound mode. Only the main zone has one.
    #[error("{} has no sound mode", .0.name())]
    NoSoundMode(Zone),
    /// The volume is not from -80 to +18 dB, or not a number.
    #[error("volume {0} dB is not from -80 to 18 dB")]
    VolumeOutOfRange(f32),
    /// The bass or treble level is not from -6 to +6 dB.
    #[error("tone level {0} dB is not from -6 to 6 dB")]
    ToneOutOfRange(i8),
    /// A setting is not valid.
    #[error("invalid setting: {0}")]
    InvalidConfig(&'static str),
}

impl Error {
    /// The request that failed, if the error comes from a request.
    #[must_use]
    pub fn request(&self) -> Option<&str> {
        match self {
            Self::Timeout { request, .. }
            | Self::Network { request, .. }
            | Self::Status { request, .. }
            | Self::Forbidden { request }
            | Self::InvalidResponse { request, .. }
            | Self::Protocol { request, .. }
            | Self::ResponseTooLarge { request, .. } => Some(request),
            Self::NotConnected
            | Self::InvalidCommand(_)
            | Self::InvalidRequest(_)
            | Self::NoSuchZone(_)
            | Self::NoSoundMode(_)
            | Self::VolumeOutOfRange(_)
            | Self::ToneOutOfRange(_)
            | Self::InvalidConfig(_) => None,
        }
    }

    /// Returns true if nothing was sent because the telnet connection is not
    /// open.
    #[must_use]
    pub const fn is_not_connected(&self) -> bool {
        matches!(self, Self::NotConnected)
    }

    /// Returns true if the caller gave something that cannot be sent.
    #[must_use]
    pub const fn is_invalid_input(&self) -> bool {
        matches!(
            self,
            Self::InvalidCommand(_)
                | Self::InvalidRequest(_)
                | Self::NoSuchZone(_)
                | Self::NoSoundMode(_)
                | Self::VolumeOutOfRange(_)
                | Self::ToneOutOfRange(_)
                | Self::InvalidConfig(_)
        )
    }

    /// Returns true if the receiver did not answer in time.
    #[must_use]
    pub const fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout { .. })
    }

    /// Returns true if the receiver could not be reached: it did not answer in
    /// time or the connection failed. Another try can work.
    #[must_use]
    pub const fn is_unreachable(&self) -> bool {
        self.is_timeout() || self.is_network()
    }

    /// Returns true if the connection failed. Another try can work.
    #[must_use]
    pub const fn is_network(&self) -> bool {
        matches!(self, Self::Network { .. })
    }

    /// Returns true if the receiver answered with an HTTP error status.
    #[must_use]
    pub const fn is_status(&self) -> bool {
        matches!(self, Self::Status { .. } | Self::Forbidden { .. })
    }

    /// Returns true if the receiver refused the request (HTTP 403).
    #[must_use]
    pub const fn is_forbidden(&self) -> bool {
        matches!(self, Self::Forbidden { .. })
    }

    /// Returns true if the receiver answered, but not in a way that makes sense.
    #[must_use]
    pub const fn is_invalid_response(&self) -> bool {
        matches!(
            self,
            Self::InvalidResponse { .. } | Self::Protocol { .. } | Self::ResponseTooLarge { .. }
        )
    }
}
