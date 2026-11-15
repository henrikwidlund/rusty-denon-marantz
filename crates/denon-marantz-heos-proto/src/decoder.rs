//! Splits the HEOS byte stream into lines.

/// Longest line that is kept. A longer line is dropped. A queue answer with
/// 100 items is about 50 KiB.
const MAX_LINE: usize = 256 * 1024;

/// Splits bytes from the CLI into lines that end with `\r\n`.
///
/// A line feed alone also ends a line. Empty lines are skipped. In a line that
/// is not valid UTF-8, each invalid part becomes `U+FFFD`.
#[derive(Debug, Default)]
pub struct LineDecoder {
    pending: Vec<u8>,
    dropping: bool,
}

impl LineDecoder {
    /// Creates an empty decoder.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending: Vec::new(),
            dropping: false,
        }
    }

    /// Adds received bytes and calls `on_line` for each complete line.
    ///
    /// The line does not include `\r\n`. A line that spans several calls is
    /// copied once. Lines inside one call are not copied.
    pub fn decode(&mut self, data: &[u8], mut on_line: impl FnMut(&str)) {
        let mut rest = data;
        while let Some(end) = rest.iter().position(|&byte| byte == b'\n') {
            let (line, tail) = (&rest[..end], &rest[end + 1..]);
            rest = tail;
            if self.pending.is_empty() && !self.dropping {
                if line.len() <= MAX_LINE {
                    emit(line, &mut on_line);
                }
                continue;
            }
            if !self.dropping && self.pending.len() + line.len() <= MAX_LINE {
                self.pending.extend_from_slice(line);
                emit(&self.pending, &mut on_line);
            }
            self.pending.clear();
            self.dropping = false;
        }
        if self.dropping {
            return;
        }
        if self.pending.len() + rest.len() > MAX_LINE {
            self.pending.clear();
            self.dropping = true;
        } else {
            self.pending.extend_from_slice(rest);
        }
    }

    /// Forgets a partial line. Call this when the connection is closed.
    pub fn reset(&mut self) {
        self.pending.clear();
        self.dropping = false;
    }
}

/// Removes the carriage return. Only a line that is not valid UTF-8 is copied.
fn emit(line: &[u8], on_line: &mut impl FnMut(&str)) {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    if !line.is_empty() {
        on_line(&String::from_utf8_lossy(line));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: [&str; 0] = [];

    fn lines(decoder: &mut LineDecoder, data: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        decoder.decode(data, |line| out.push(line.to_owned()));
        out
    }

    #[test]
    fn splits_lines_in_one_chunk() {
        let mut decoder = LineDecoder::new();
        assert_eq!(
            lines(&mut decoder, b"{\"a\":1}\r\n{}\r\n"),
            ["{\"a\":1}", "{}"]
        );
    }

    #[test]
    fn joins_a_line_split_over_chunks() {
        let mut decoder = LineDecoder::new();
        assert_eq!(lines(&mut decoder, b"{\"a\""), NONE);
        assert_eq!(lines(&mut decoder, b":1}\r"), NONE);
        assert_eq!(lines(&mut decoder, b"\n{}"), ["{\"a\":1}"]);
        assert_eq!(lines(&mut decoder, b"\r\n"), ["{}"]);
    }

    #[test]
    fn line_feed_alone_ends_a_line() {
        let mut decoder = LineDecoder::new();
        assert_eq!(lines(&mut decoder, b"{}\n[]\n"), ["{}", "[]"]);
    }

    #[test]
    fn skips_empty_lines() {
        let mut decoder = LineDecoder::new();
        assert_eq!(lines(&mut decoder, b"\r\n\n{}\r\n\r\n"), ["{}"]);
    }

    #[test]
    fn replaces_invalid_utf8() {
        let mut decoder = LineDecoder::new();
        assert_eq!(
            lines(&mut decoder, b"\"Bj\xf6rk\"\r\n{}\r\n"),
            ["\"Bj\u{fffd}rk\"", "{}"]
        );
    }

    #[test]
    fn drops_a_line_that_is_too_long() {
        let mut decoder = LineDecoder::new();
        let long = vec![b'A'; MAX_LINE + 1];
        assert_eq!(lines(&mut decoder, &long), NONE);
        assert_eq!(lines(&mut decoder, &long), NONE);
        assert_eq!(lines(&mut decoder, b"AA\r\n{}\r\n"), ["{}"]);
        let mut complete = long;
        complete.extend_from_slice(b"\n{}\n");
        assert_eq!(lines(&mut decoder, &complete), ["{}"]);
    }

    #[test]
    fn keeps_a_line_of_the_longest_size_with_its_carriage_return() {
        let mut decoder = LineDecoder::new();
        let long = vec![b'A'; MAX_LINE - 1];
        assert_eq!(lines(&mut decoder, &long), NONE);
        assert_eq!(
            lines(&mut decoder, b"\r\n"),
            [String::from_utf8(long).unwrap()]
        );
    }

    #[test]
    fn reset_forgets_a_partial_line() {
        let mut decoder = LineDecoder::new();
        assert_eq!(lines(&mut decoder, b"{\"a\""), NONE);
        decoder.reset();
        assert_eq!(lines(&mut decoder, b"{}\r\n"), ["{}"]);
    }

    proptest::proptest! {
        #[test]
        fn chunking_does_not_change_the_lines(
            data in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..300),
            cuts in proptest::collection::vec(0_usize..300, 0..6),
        ) {
            let mut whole = LineDecoder::new();
            let expected = lines(&mut whole, &data);

            let mut cuts: Vec<usize> = cuts.into_iter().map(|c| c.min(data.len())).collect();
            cuts.sort_unstable();
            let mut split = LineDecoder::new();
            let mut actual = Vec::new();
            let mut start = 0;
            for cut in cuts.into_iter().chain([data.len()]) {
                actual.extend(lines(&mut split, &data[start..cut]));
                start = cut;
            }
            proptest::prop_assert_eq!(actual, expected);
        }
    }
}
