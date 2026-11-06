//! Errors of the client.

use std::time::Duration;

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
    /// The host or the path is not valid.
    #[error("invalid request: {0}")]
    InvalidRequest(String),
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
            Self::InvalidRequest(_) | Self::InvalidConfig(_) => None,
        }
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
