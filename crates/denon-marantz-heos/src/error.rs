//! Errors of the client.

use std::time::Duration;

use denon_marantz_heos_proto::Failure;

/// Error of the client.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// The device did not answer in time.
    #[error("{request}: no answer within {limit:?}")]
    Timeout {
        /// The request, for example `player/get_play_state`.
        request: String,
        /// The time that was allowed.
        limit: Duration,
        /// The command may have reached the device. `false` if it surely did
        /// not: the connection could not be opened. See
        /// [`Error::is_not_sent`].
        sent: bool,
    },
    /// The connection could not be opened, so nothing was sent.
    #[error("{request}: network error: {source}")]
    Network {
        /// The request, for example `connect 192.168.1.20:1255`.
        request: String,
        /// Cause.
        #[source]
        source: std::io::Error,
    },
    /// The connection is not open, so nothing was sent. For
    /// [`Heos::refresh`](crate::Heos::refresh) and the first read of a
    /// connect, a read may have been sent.
    #[error("the HEOS connection is not open")]
    NotConnected,
    /// The connection ended while the command was written or before its
    /// answer came. The device may have done the command.
    #[error("{command}: the HEOS connection ended before the answer")]
    Disconnected {
        /// The command, for example `player/set_play_state`.
        command: &'static str,
    },
    /// Too many commands wait to be sent. Nothing was sent.
    #[error("too many HEOS commands wait to be sent")]
    QueueFull,
    /// The device answered that the command failed.
    #[error("{command} failed: {}", FailureText(failure))]
    Failed {
        /// The command, for example `player/set_play_state`.
        command: &'static str,
        /// Details from the device.
        failure: Failure<'static>,
    },
    /// The answer is not what the protocol allows.
    #[error("{command}: {source}")]
    Protocol {
        /// The command, for example `player/get_players`.
        command: &'static str,
        /// Cause.
        #[source]
        source: denon_marantz_heos_proto::Error,
    },
    /// A command cannot be sent: text with a control character, or a value
    /// out of range. Nothing was sent.
    #[error("invalid command: {0}")]
    InvalidCommand(#[source] denon_marantz_heos_proto::Error),
    /// A setting is not valid.
    #[error("invalid setting: {0}")]
    InvalidConfig(&'static str),
}

impl Error {
    /// Returns true if nothing was sent because the connection is not open.
    #[must_use]
    pub const fn is_not_connected(&self) -> bool {
        matches!(self, Self::NotConnected)
    }

    /// Returns true if the command surely did not reach the device: the
    /// connection is not open or could not be opened, or too many commands
    /// wait. Sending it again cannot run it twice. It is only for errors of
    /// the connection: others, for example invalid input, give false. For
    /// [`Heos::refresh`](crate::Heos::refresh), a read may have been sent;
    /// another read does no harm.
    #[must_use]
    pub const fn is_not_sent(&self) -> bool {
        matches!(
            self,
            Self::NotConnected
                | Self::QueueFull
                | Self::Network { .. }
                | Self::Timeout { sent: false, .. }
        )
    }

    /// Returns true if the device did not answer in time.
    #[must_use]
    pub const fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout { .. })
    }

    /// Returns true if the connection could not be opened, or it ended
    /// before the answer.
    #[must_use]
    pub const fn is_network(&self) -> bool {
        matches!(self, Self::Network { .. } | Self::Disconnected { .. })
    }

    /// Returns true if another try can work: the device did not answer in
    /// time or could not be reached, the connection ended before the answer,
    /// the connection is not open now, or too many commands wait.
    #[must_use]
    pub const fn is_transient(&self) -> bool {
        self.is_timeout()
            || self.is_network()
            || matches!(self, Self::NotConnected | Self::QueueFull)
    }

    /// Returns true if the device answered that the command failed.
    #[must_use]
    pub const fn is_rejected(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }

    /// Returns true if the answer is not what the protocol allows.
    #[must_use]
    pub const fn is_invalid_response(&self) -> bool {
        matches!(self, Self::Protocol { .. })
    }

    /// Returns true if the caller gave a value that cannot be sent. Nothing
    /// was sent.
    #[must_use]
    pub const fn is_invalid_input(&self) -> bool {
        matches!(self, Self::InvalidCommand(_))
    }

    /// Returns true if the settings are not valid.
    #[must_use]
    pub const fn is_invalid_config(&self) -> bool {
        matches!(self, Self::InvalidConfig(_))
    }
}

/// Error code and text of a failure, for the error message.
pub(crate) struct FailureText<'a>(pub(crate) &'a Failure<'a>);

impl std::fmt::Display for FailureText<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Failure {
            code,
            text,
            system_code,
        } = self.0;
        match (code, text.is_empty()) {
            (Some(code), true) => write!(f, "error {}", code.id()),
            (Some(code), false) => write!(f, "error {}: {text}", code.id()),
            (None, false) => f.write_str(text),
            (None, true) => f.write_str("no details"),
        }?;
        if let Some(number) = system_code {
            write!(f, " (syserrno {number})")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use denon_marantz_heos_proto::ErrorCode;

    use super::*;

    #[test]
    fn failure_message_has_the_code_and_the_text() {
        let failed = |code, text: &'static str, system_code| Error::Failed {
            command: "player/play_next",
            failure: Failure {
                code,
                text: text.into(),
                system_code,
            },
        };
        let cases = [
            (
                Some(ErrorCode::InvalidId),
                "ID Not Valid",
                None,
                "player/play_next failed: error 2: ID Not Valid",
            ),
            (
                Some(ErrorCode::InvalidId),
                "",
                None,
                "player/play_next failed: error 2",
            ),
            (
                Some(ErrorCode::SystemError),
                "System error",
                Some(-9),
                "player/play_next failed: error 12: System error (syserrno -9)",
            ),
            (None, "Busy", None, "player/play_next failed: Busy"),
            (None, "", None, "player/play_next failed: no details"),
        ];
        for (code, text, system_code, expected) in cases {
            assert_eq!(failed(code, text, system_code).to_string(), expected);
        }
    }

    #[test]
    fn checks_match_the_variants() {
        let timeout = |sent| Error::Timeout {
            request: "connect".to_owned(),
            limit: Duration::from_secs(1),
            sent,
        };
        let failed = Error::Failed {
            command: "player/play_next",
            failure: Failure {
                code: None,
                text: "".into(),
                system_code: None,
            },
        };
        // not sent, timeout, network, transient, rejected, invalid response,
        // invalid input
        let cases = [
            (
                timeout(false),
                [true, true, false, true, false, false, false],
            ),
            (
                timeout(true),
                [false, true, false, true, false, false, false],
            ),
            (
                Error::Network {
                    request: "connect".to_owned(),
                    source: std::io::Error::from(std::io::ErrorKind::ConnectionRefused),
                },
                [true, false, true, true, false, false, false],
            ),
            (
                Error::NotConnected,
                [true, false, false, true, false, false, false],
            ),
            (
                Error::Disconnected {
                    command: "player/play_next",
                },
                [false, false, true, true, false, false, false],
            ),
            (
                Error::QueueFull,
                [true, false, false, true, false, false, false],
            ),
            (failed, [false, false, false, false, true, false, false]),
            (
                Error::Protocol {
                    command: "player/get_players",
                    source: denon_marantz_heos_proto::Error::NoPayload,
                },
                [false, false, false, false, false, true, false],
            ),
            (
                Error::InvalidCommand(denon_marantz_heos_proto::Error::InvalidCommandText),
                [false, false, false, false, false, false, true],
            ),
            (
                Error::InvalidConfig("x"),
                [false, false, false, false, false, false, false],
            ),
        ];
        for (error, expected) in cases {
            let found = [
                error.is_not_sent(),
                error.is_timeout(),
                error.is_network(),
                error.is_transient(),
                error.is_rejected(),
                error.is_invalid_response(),
                error.is_invalid_input(),
            ];
            assert_eq!(found, expected, "{error}");
        }
        assert!(Error::NotConnected.is_not_connected());
        assert!(!Error::QueueFull.is_not_connected());
        assert!(Error::InvalidConfig("x").is_invalid_config());
        assert!(!Error::NotConnected.is_invalid_config());
    }
}
