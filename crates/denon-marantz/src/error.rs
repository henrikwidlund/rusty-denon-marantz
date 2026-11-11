//! Errors of the client.

use std::time::Duration;

use denon_marantz_proto::Zone;

type Source = Box<dyn std::error::Error + Send + Sync>;

/// What stops tone control from turning on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToneBlocker {
    /// A Dirac Live filter is selected.
    DiracFilter,
}

impl std::fmt::Display for ToneBlocker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::DiracFilter => "a Dirac filter is selected",
        })
    }
}

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
    /// The receiver answered, but nothing was read: every page that was asked
    /// for was missing, refused or not readable.
    #[error("nothing was read: no page that was asked for could be read")]
    NothingRead,
    /// The library does not know the code of this sound mode of the list, so
    /// nothing was sent. Select it with the code, see
    /// `ZoneControl::select_sound_mode`.
    #[error("the library has no code for the sound mode {0}")]
    UnknownSoundMode(String),
    /// The runtime stopped the task of a start before it was finished. It
    /// is only in the report of that start.
    #[error("the start was stopped before it finished")]
    Stopped,
    /// Another start of this receiver is still running, so nothing was read.
    /// A start is running until [`Started::finished`](crate::Started::finished)
    /// would return.
    #[error("another start of this receiver is still running")]
    StartRunning,
    /// The telnet connection is not open, so nothing was sent.
    ///
    /// After a write was begun, the line may still have been sent.
    #[error("the telnet connection is not open")]
    NotConnected,
    /// A command cannot be sent, for example because its text has a control
    /// character.
    #[error("invalid command: {0}")]
    InvalidCommand(#[source] denon_marantz_proto::Error),
    /// A number for a command is outside the range that the receiver
    /// accepts. Nothing was sent.
    #[error("{what} is out of range")]
    NumberOutOfRange {
        /// The setting.
        what: &'static str,
    },
    /// The receiver or the zone does not have this: a remote key of another
    /// brand, or a setting that only zone 2 and zone 3 have. Nothing was sent.
    #[error("not supported: {0}")]
    Unsupported(#[source] denon_marantz_proto::Error),
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
    /// The containment amount is not from 1 to 7.
    #[error("containment amount {0} is not from 1 to 7")]
    ContainmentOutOfRange(u8),
    /// Tone control would not turn on. The receiver ignores the command and
    /// sends no error. Nothing was sent.
    #[error("tone control cannot be turned on: {0}")]
    ToneControlBlocked(ToneBlocker),
    /// An Audyssey setting cannot be used while a Dirac Live filter is
    /// selected: Dirac and Audyssey do not work together, and the receiver
    /// ignores the setting. Nothing was sent. Turn the Dirac filter off, or
    /// select an Audyssey MultEQ mode, first.
    #[error("Audyssey settings are ignored while a Dirac filter is selected")]
    AudysseyBlockedByDirac,
    /// The reference level offset works only with Dynamic EQ on. Nothing was
    /// sent.
    #[error("the reference level offset needs Dynamic EQ to be on")]
    DynamicEqOff,
    /// The Dirac Live slot is not 1, 2 or 3.
    #[error("Dirac slot {0} is not 1, 2 or 3")]
    DiracSlotOutOfRange(u8),
    /// No Dirac Live slot has this name, as far as the state knows.
    #[error("no Dirac slot is named {0:?}")]
    UnknownDiracSlot(String),
    /// The receiver does not support Dirac Live.
    #[error("the receiver does not support Dirac Live")]
    DiracNotSupported,
    /// A setting is not valid.
    #[error("invalid setting: {0}")]
    InvalidConfig(&'static str),
    /// The key for a factory reset is not the last one from `reset_key`, it
    /// was used, or it is older than 60 seconds.
    #[error("the reset key is not valid")]
    ResetKeyInvalid,
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
            Self::Stopped
            | Self::StartRunning
            | Self::NothingRead
            | Self::NotConnected
            | Self::InvalidCommand(_)
            | Self::NumberOutOfRange { .. }
            | Self::Unsupported(_)
            | Self::InvalidRequest(_)
            | Self::NoSuchZone(_)
            | Self::NoSoundMode(_)
            | Self::VolumeOutOfRange(_)
            | Self::ToneOutOfRange(_)
            | Self::ContainmentOutOfRange(_)
            | Self::ToneControlBlocked(_)
            | Self::DynamicEqOff
            | Self::AudysseyBlockedByDirac
            | Self::DiracSlotOutOfRange(_)
            | Self::UnknownDiracSlot(_)
            | Self::UnknownSoundMode(_)
            | Self::DiracNotSupported
            | Self::InvalidConfig(_)
            | Self::ResetKeyInvalid => None,
        }
    }

    /// Returns true if nothing was sent because the telnet connection is not
    /// open.
    #[must_use]
    pub const fn is_not_connected(&self) -> bool {
        matches!(self, Self::NotConnected)
    }

    /// Returns true if the caller gave a value that cannot be sent: text with
    /// a control character, a number out of range, an unknown name or a bad
    /// request. Nothing was sent.
    #[must_use]
    pub const fn is_invalid_input(&self) -> bool {
        matches!(
            self,
            Self::InvalidCommand(_)
                | Self::InvalidRequest(_)
                | Self::NumberOutOfRange { .. }
                | Self::VolumeOutOfRange(_)
                | Self::ToneOutOfRange(_)
                | Self::ContainmentOutOfRange(_)
                | Self::DiracSlotOutOfRange(_)
                | Self::UnknownDiracSlot(_)
                | Self::UnknownSoundMode(_)
                | Self::ResetKeyInvalid
        )
    }

    /// Returns true if the receiver is in a state that does not allow the
    /// setting: tone control, Audyssey or the reference level offset. Nothing
    /// was sent. The error names the reason. Change the other setting first.
    #[must_use]
    pub const fn is_blocked(&self) -> bool {
        matches!(
            self,
            Self::ToneControlBlocked(_) | Self::AudysseyBlockedByDirac | Self::DynamicEqOff
        )
    }

    /// Returns true if this receiver or zone does not have the function:
    /// Dirac Live, a zone, the sound mode of a zone or a remote key. Another
    /// try gives the same error. Nothing was sent.
    #[must_use]
    pub const fn is_unsupported(&self) -> bool {
        matches!(
            self,
            Self::Unsupported(_)
                | Self::NoSuchZone(_)
                | Self::NoSoundMode(_)
                | Self::DiracNotSupported
        )
    }

    /// Returns true if the settings of the builder are not valid.
    #[must_use]
    pub const fn is_invalid_config(&self) -> bool {
        matches!(self, Self::InvalidConfig(_))
    }

    /// Returns true if another try can work: the receiver could not be
    /// reached, or the telnet connection is not open now and the task opens
    /// it again.
    #[must_use]
    pub const fn is_transient(&self) -> bool {
        self.is_unreachable() || self.is_not_connected()
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

    /// Returns true if the receiver answered, but not in a way that makes
    /// sense, or nothing that was asked for could be read.
    #[must_use]
    pub const fn is_invalid_response(&self) -> bool {
        matches!(
            self,
            Self::InvalidResponse { .. }
                | Self::Protocol { .. }
                | Self::ResponseTooLarge { .. }
                | Self::NothingRead
        )
    }

    /// The error for a command that the library refused to build.
    #[cfg(feature = "telnet")]
    pub(crate) fn from_command(source: denon_marantz_proto::Error) -> Self {
        use denon_marantz_proto::Error as Proto;
        match source {
            Proto::ValueOutOfRange { what } => Self::NumberOutOfRange { what },
            Proto::UnsupportedKey | Proto::UnsupportedZone => Self::Unsupported(source),
            _ => Self::InvalidCommand(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_named_only_by_errors_of_a_request() {
        let status = Error::Status {
            request: "GET http://10.0.0.5/x".to_owned(),
            status: 500,
        };
        assert_eq!(status.request(), Some("GET http://10.0.0.5/x"));
        assert!(status.is_status());
        assert!(!status.is_unreachable());
        let timeout = Error::Timeout {
            request: "GET http://10.0.0.5/y".to_owned(),
            limit: Duration::from_secs(1),
        };
        assert_eq!(timeout.request(), Some("GET http://10.0.0.5/y"));
        assert!(timeout.is_timeout() && timeout.is_unreachable());
        for error in [
            Error::NotConnected,
            Error::Stopped,
            Error::StartRunning,
            Error::DiracNotSupported,
        ] {
            assert_eq!(error.request(), None, "{error}");
        }
        assert!(Error::NotConnected.is_not_connected());
    }

    #[test]
    fn input_errors_are_told_apart() {
        for error in [
            Error::DiracSlotOutOfRange(4),
            Error::UnknownDiracSlot("x".to_owned()),
            Error::NumberOutOfRange {
                what: "quick select",
            },
        ] {
            assert!(error.is_invalid_input(), "{error}");
            assert!(!error.is_blocked() && !error.is_unsupported(), "{error}");
        }
        let blocked = Error::ToneControlBlocked(ToneBlocker::DiracFilter);
        assert!(blocked.is_blocked() && !blocked.is_invalid_input());
        assert!(Error::AudysseyBlockedByDirac.is_blocked());
        assert!(Error::DynamicEqOff.is_blocked());
        for error in [
            Error::DiracNotSupported,
            Error::NoSoundMode(Zone::Zone2),
            Error::NoSuchZone(Zone::Zone3),
        ] {
            assert!(
                error.is_unsupported() && !error.is_invalid_input(),
                "{error}"
            );
        }
        assert!(Error::InvalidConfig("x").is_invalid_config());
        assert!(!Error::InvalidConfig("x").is_invalid_input());
        assert_eq!(
            ToneBlocker::DiracFilter.to_string(),
            "a Dirac filter is selected"
        );
        assert!(!Error::Stopped.is_invalid_input());
    }

    #[cfg(feature = "telnet")]
    #[test]
    fn a_command_error_has_the_variant_of_its_reason() {
        use denon_marantz_proto::Error as Proto;
        assert!(matches!(
            Error::from_command(Proto::ValueOutOfRange { what: "level" }),
            Error::NumberOutOfRange { what: "level" }
        ));
        assert!(Error::from_command(Proto::UnsupportedKey).is_unsupported());
        assert!(Error::from_command(Proto::UnsupportedZone).is_unsupported());
        let text = Error::from_command(Proto::InvalidCommandText);
        assert!(matches!(text, Error::InvalidCommand(_)) && text.is_invalid_input());
    }

    #[test]
    fn transient_errors_can_work_on_another_try() {
        assert!(Error::NotConnected.is_transient());
        let timeout = Error::Timeout {
            request: "x".to_owned(),
            limit: Duration::from_secs(1),
        };
        assert!(timeout.is_transient());
        assert!(!Error::DiracNotSupported.is_transient());
        assert!(Error::NothingRead.is_invalid_response());
    }
}
