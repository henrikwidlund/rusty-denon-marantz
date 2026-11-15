//! The `message` text of answers and events, and the encoding of its values.

use std::borrow::Cow;

/// Fields of a `message` text: `key=value` parts, separated by `&`.
///
/// A part can be a key without a value, for example `signed_out` or
/// `command under process`. Values are encoded: [`Message::get`] decodes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Message<'a>(&'a str);

impl<'a> Message<'a> {
    /// Wraps a `message` text.
    #[must_use]
    pub const fn new(text: &'a str) -> Self {
        Self(text)
    }

    /// The text as received.
    #[must_use]
    pub const fn as_str(&self) -> &'a str {
        self.0
    }

    /// Key and encoded value of each part. Empty parts are skipped.
    pub fn fields(&self) -> impl Iterator<Item = (&'a str, Option<&'a str>)> + use<'a> {
        self.0
            .split('&')
            .filter(|part| !part.is_empty())
            .map(|part| match part.split_once('=') {
                Some((key, value)) => (key, Some(value)),
                None => (part, None),
            })
    }

    /// Decoded value of the first part with this key.
    ///
    /// Returns `None` if there is no such key or the key has no value.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<Cow<'a, str>> {
        self.fields()
            .find(|&(name, _)| name == key)
            .and_then(|(_, value)| value)
            .map(decode)
    }

    /// Returns true if a part has this key, with or without a value.
    #[must_use]
    pub fn has(&self, key: &str) -> bool {
        self.fields().any(|(name, _)| name == key)
    }
}

/// Decodes `%26`, `%3D` and `%25` to `&`, `=` and `%`.
///
/// Other `%` sequences stay as they are. Copies only if there is something to
/// decode.
#[must_use]
pub fn decode(value: &str) -> Cow<'_, str> {
    if !value.contains('%') {
        return Cow::Borrowed(value);
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(index) = rest.find('%') {
        out.push_str(&rest[..index]);
        let tail = &rest[index..];
        let decoded = match tail.get(1..3) {
            Some("26") => Some('&'),
            Some("3D" | "3d") => Some('='),
            Some("25") => Some('%'),
            _ => None,
        };
        if let Some(char) = decoded {
            out.push(char);
            rest = &tail[3..];
        } else {
            out.push('%');
            rest = &tail[1..];
        }
    }
    out.push_str(rest);
    Cow::Owned(out)
}

/// Appends `value` with `&`, `=` and `%` encoded.
pub(crate) fn push_encoded(out: &mut String, value: &str) {
    let mut rest = value;
    while let Some(index) = rest.find(['&', '=', '%']) {
        out.push_str(&rest[..index]);
        out.push_str(match rest.as_bytes()[index] {
            b'&' => "%26",
            b'=' => "%3D",
            _ => "%25",
        });
        rest = &rest[index + 1..];
    }
    out.push_str(rest);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_fields() {
        let message = Message::new("pid=1&state=play&&flag");
        assert_eq!(
            message.fields().collect::<Vec<_>>(),
            [("pid", Some("1")), ("state", Some("play")), ("flag", None)]
        );
        assert_eq!(message.get("state").as_deref(), Some("play"));
        assert_eq!(message.get("flag"), None);
        assert_eq!(message.get("missing"), None);
        assert!(message.has("flag"));
        assert!(!message.has("missing"));
    }

    #[test]
    fn value_can_contain_an_equals_sign() {
        let message = Message::new("url=http://host/a?b=c");
        assert_eq!(message.get("url").as_deref(), Some("http://host/a?b=c"));
    }

    #[test]
    fn decodes_values() {
        let message = Message::new("error=50%25 %26 more %3D less%2");
        assert_eq!(message.get("error").as_deref(), Some("50% & more = less%2"));
        assert!(matches!(decode("plain"), Cow::Borrowed("plain")));
        assert_eq!(decode("%20%3d%"), "%20=%");
        assert_eq!(decode("%2526"), "%26");
    }

    #[test]
    fn empty_message_has_no_fields() {
        assert_eq!(Message::new("").fields().count(), 0);
    }

    #[test]
    fn encodes_values() {
        let mut out = String::new();
        push_encoded(&mut out, "a&b=c%d é");
        assert_eq!(out, "a%26b%3Dc%25d é");
    }

    proptest::proptest! {
        #[test]
        fn decode_reverses_encode(value in "\\PC*") {
            let mut encoded = String::new();
            push_encoded(&mut encoded, &value);
            proptest::prop_assert_eq!(decode(&encoded), value.as_str());
        }
    }
}
