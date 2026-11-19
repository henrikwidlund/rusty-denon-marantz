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
    },
    /// The connection could not be opened.
    #[error("{request}: network error: {source}")]
    Network {
        /// The request, for example `connect 192.168.1.20:1255`.
        request: String,
        /// Cause.
        #[source]
        source: std::io::Error,
    },
    /// The connection is not open, so nothing was sent.
    ///
    /// If the connection was lost after the command was written, the device
    /// may still have done it.
    #[error("the HEOS connection is not open")]
    NotConnected,
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

    /// Returns true if the caller gave a value that cannot be sent. Nothing
    /// was sent.
    #[must_use]
    pub const fn is_invalid_input(&self) -> bool {
        matches!(self, Self::InvalidCommand(_) | Self::InvalidConfig(_))
    }
}

/// Error code and text of a failure, for the error message.
struct FailureText<'a>(&'a Failure<'a>);

impl std::fmt::Display for FailureText<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let Failure { code, text, .. } = self.0;
        match (code, text.is_empty()) {
            (Some(code), true) => write!(f, "error {}", code.id()),
            (Some(code), false) => write!(f, "error {}: {text}", code.id()),
            (None, false) => f.write_str(text),
            (None, true) => f.write_str("no details"),
        }
    }
}

#[cfg(test)]
mod tests {
    use denon_marantz_heos_proto::ErrorCode;

    use super::*;

    #[test]
    fn failure_message_has_the_code_and_the_text() {
        let failed = |code, text: &'static str| Error::Failed {
            command: "player/play_next",
            failure: Failure {
                code,
                text: text.into(),
                system_code: None,
            },
        };
        let cases = [
            (
                Some(ErrorCode::InvalidId),
                "ID Not Valid",
                "player/play_next failed: error 2: ID Not Valid",
            ),
            (
                Some(ErrorCode::InvalidId),
                "",
                "player/play_next failed: error 2",
            ),
            (None, "Busy", "player/play_next failed: Busy"),
            (None, "", "player/play_next failed: no details"),
        ];
        for (code, text, expected) in cases {
            assert_eq!(failed(code, text).to_string(), expected);
        }
    }
}
