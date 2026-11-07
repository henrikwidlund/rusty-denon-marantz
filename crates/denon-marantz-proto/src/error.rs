//! Errors of this crate.

/// Error of this crate.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Text for a command is empty or has a control character. A carriage
    /// return would end the command and start a new one.
    #[error("command text is empty or contains a control character")]
    InvalidCommandText,
    /// A number for a command is outside the range that the receiver
    /// accepts.
    #[error("{what} is out of range")]
    ValueOutOfRange {
        /// The setting.
        what: &'static str,
    },
    /// The brand or the zone of the receiver has no such remote key.
    #[error("the receiver has no such remote key")]
    UnsupportedKey,
    /// The setting exists for zone 2 and zone 3 only.
    #[error("the setting is not available for this zone")]
    UnsupportedZone,
    /// A device description is not from a supported receiver.
    #[error("the device description is not from a Denon or Marantz receiver")]
    UnsupportedDevice,
    /// The name of an XML element is not valid.
    #[error("XML element name is not valid")]
    InvalidName,
    /// A document is not valid XML or has a DTD.
    #[error("invalid XML ({what}) at byte {position}, document starts with {start}")]
    InvalidXml {
        /// What is wrong.
        what: &'static str,
        /// Byte position of the problem.
        position: usize,
        /// First bytes of the document as hex.
        start: String,
    },
    /// The receiver returned an HTML page instead of XML.
    #[error("response is an HTML page")]
    HtmlResponse,
    /// The response has a different number of items than the request.
    #[error("response has {actual} items, request has {expected}")]
    UnexpectedItemCount {
        /// Number of commands in the request.
        expected: usize,
        /// Number of items in the response.
        actual: usize,
    },
    /// The response has an element other than `cmd` or `error`.
    #[error("response has an unexpected element at item {index}")]
    UnexpectedElement {
        /// Index of the item.
        index: usize,
    },
}

impl Error {
    /// Returns true if the error comes from text that the caller gave.
    #[must_use]
    pub const fn is_invalid_input(&self) -> bool {
        matches!(
            self,
            Self::InvalidCommandText
                | Self::InvalidName
                | Self::ValueOutOfRange { .. }
                | Self::UnsupportedKey
                | Self::UnsupportedZone
        )
    }
}
