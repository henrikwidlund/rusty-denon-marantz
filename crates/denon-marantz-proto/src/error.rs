//! Errors of this crate.

/// Error of this crate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Text for a command is empty or has a control character. A carriage
    /// return would end the command and start a new one.
    #[error("command text is empty or contains a control character")]
    InvalidCommandText,
}

impl Error {
    /// Returns true if the error comes from text that the caller gave.
    #[must_use]
    pub const fn is_invalid_input(&self) -> bool {
        matches!(self, Self::InvalidCommandText)
    }
}
