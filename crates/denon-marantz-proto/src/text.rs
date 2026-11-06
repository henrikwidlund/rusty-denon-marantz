//! Checks for text that is sent to a receiver.

use crate::error::Error;

/// Rejects text with a control character.
///
/// In a telnet command a carriage return ends the command, so it could start
/// a second one. XML does not allow control characters in text.
pub(crate) fn reject_control(text: &str) -> Result<(), Error> {
    if text.chars().any(char::is_control) {
        Err(Error::InvalidCommandText)
    } else {
        Ok(())
    }
}
