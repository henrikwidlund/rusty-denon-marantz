//! Helpers for the tests that read expected values from data files.

#![allow(dead_code)] // Each test file uses a part of this.

use std::fs;
use std::path::PathBuf;

/// Marks a missing value in the data files.
pub(crate) const NONE: &str = "<none>";

/// Path of a file in `tests/data`.
pub(crate) fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

/// Text of a data file.
pub(crate) fn read_data(name: &str) -> String {
    fs::read_to_string(data(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Bytes of a data file.
pub(crate) fn read_bytes(name: &str) -> Vec<u8> {
    fs::read(data(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

/// Reads a value from a data file. `<none>` is `None`. `\t`, `\n`, `\r` and
/// `\\` are escapes.
pub(crate) fn unescape(text: &str) -> Option<String> {
    if text == NONE {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(match chars.next() {
                Some('t') => '\t',
                Some('n') => '\n',
                Some('r') => '\r',
                Some(other) => other,
                None => '\\',
            });
        } else {
            out.push(c);
        }
    }
    Some(out)
}
