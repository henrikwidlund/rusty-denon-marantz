//! Errors of this crate.

/// Error of this crate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Text for a command is empty or has a control character. A line break
    /// would end the command and start a new one.
    #[error("command text is empty or contains a control character")]
    InvalidCommandText,
    /// A number for a command is outside the range that the CLI accepts.
    #[error("{what} is out of range")]
    ValueOutOfRange {
        /// The value.
        what: &'static str,
    },
    /// A line is not valid JSON or does not have the expected fields.
    #[error("invalid JSON: {what}")]
    InvalidJson {
        /// What is wrong.
        what: String,
        /// Line of the problem, starting at 1. It is 0 if not known.
        line: usize,
        /// Column of the problem, starting at 1. It is 0 if not known.
        column: usize,
    },
    /// The message of an event has no such field.
    #[error("the message has no {field} field")]
    MissingField {
        /// Name of the field.
        field: &'static str,
    },
    /// A field of the message has a value that is not valid.
    #[error("the {field} field has a value that is not valid")]
    InvalidValue {
        /// Name of the field.
        field: &'static str,
    },
    /// The answer has no payload.
    #[error("the answer has no payload")]
    NoPayload,
}

impl Error {
    /// Returns true if the error comes from a value that the caller gave.
    #[must_use]
    pub const fn is_invalid_input(&self) -> bool {
        matches!(
            self,
            Self::InvalidCommandText | Self::ValueOutOfRange { .. }
        )
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::InvalidJson {
            what: error.to_string(),
            line: error.line(),
            column: error.column(),
        }
    }
}
