//! Splits the telnet byte stream into lines.

/// Longest line that is kept while its end is missing. A longer line is dropped.
const MAX_LINE: usize = 8192;

/// Splits bytes from the receiver into lines that end with a carriage return.
///
/// Lines that are not valid UTF-8 are dropped.
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
    /// The line does not include the carriage return. A line that spans several
    /// calls is copied once. Lines inside one call are not copied.
    pub fn decode(&mut self, data: &[u8], mut on_line: impl FnMut(&str)) {
        let mut rest = data;
        while let Some(end) = rest.iter().position(|&byte| byte == b'\r') {
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

fn emit(line: &[u8], on_line: &mut impl FnMut(&str)) {
    if let Ok(line) = core::str::from_utf8(line) {
        on_line(line);
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
            lines(&mut decoder, b"PWON\rMV43\rMUOFF\r"),
            ["PWON", "MV43", "MUOFF"]
        );
    }

    #[test]
    fn joins_a_line_split_over_chunks() {
        let mut decoder = LineDecoder::new();
        assert_eq!(lines(&mut decoder, b"PW"), NONE);
        assert_eq!(lines(&mut decoder, b"ON"), NONE);
        assert_eq!(lines(&mut decoder, b"\rMV"), ["PWON"]);
        assert_eq!(lines(&mut decoder, b"43\r"), ["MV43"]);
    }

    #[test]
    fn carriage_return_in_its_own_chunk() {
        let mut decoder = LineDecoder::new();
        assert_eq!(lines(&mut decoder, b"PWON"), NONE);
        assert_eq!(lines(&mut decoder, b"\r"), ["PWON"]);
    }

    #[test]
    fn empty_lines_are_reported() {
        let mut decoder = LineDecoder::new();
        assert_eq!(lines(&mut decoder, b"\rPWON\r"), ["", "PWON"]);
    }

    #[test]
    fn drops_invalid_utf8_and_continues() {
        let mut decoder = LineDecoder::new();
        assert_eq!(lines(&mut decoder, b"\xff\xfe\rPWON\r"), ["PWON"]);
        assert_eq!(lines(&mut decoder, b"\xff"), NONE);
        assert_eq!(lines(&mut decoder, b"\xfe\rMV43\r"), ["MV43"]);
    }

    #[test]
    fn keeps_non_ascii_text() {
        let mut decoder = LineDecoder::new();
        assert_eq!(lines(&mut decoder, "NSE1Björk\r".as_bytes()), ["NSE1Björk"]);
    }

    #[test]
    fn drops_a_line_that_is_too_long() {
        let mut decoder = LineDecoder::new();
        let long = vec![b'A'; MAX_LINE + 1];
        assert_eq!(lines(&mut decoder, &long), NONE);
        assert_eq!(lines(&mut decoder, &long), NONE);
        assert_eq!(lines(&mut decoder, b"AA\rPWON\r"), ["PWON"]);
    }

    #[test]
    fn drops_a_complete_line_that_is_too_long() {
        let mut decoder = LineDecoder::new();
        let mut long = vec![b'A'; MAX_LINE + 1];
        long.push(b'\r');
        assert_eq!(lines(&mut decoder, &long), NONE);
        assert_eq!(lines(&mut decoder, b"PWON\r"), ["PWON"]);
    }

    #[test]
    fn accepts_a_line_of_the_longest_size() {
        let mut decoder = LineDecoder::new();
        let long = vec![b'A'; MAX_LINE];
        assert_eq!(lines(&mut decoder, &long), NONE);
        assert_eq!(
            lines(&mut decoder, b"\r"),
            [String::from_utf8(long).unwrap()]
        );
    }

    #[test]
    fn long_lines_are_dropped_for_any_chunk_size() {
        let mut input = vec![b'A'; MAX_LINE + 1];
        input.extend_from_slice(b"\rPWON\r");
        let mut input_ok = vec![b'B'; MAX_LINE];
        input_ok.extend_from_slice(b"\r");
        input.extend_from_slice(&input_ok);
        let expected = ["PWON".to_owned(), "B".repeat(MAX_LINE)];
        for size in [
            1,
            7,
            1000,
            MAX_LINE,
            MAX_LINE + 1,
            MAX_LINE + 5,
            input.len(),
        ] {
            let mut decoder = LineDecoder::new();
            let mut actual = Vec::new();
            for chunk in input.chunks(size) {
                actual.extend(lines(&mut decoder, chunk));
            }
            assert_eq!(actual, expected, "chunk size {size}");
        }
    }

    #[test]
    fn reset_forgets_a_partial_line() {
        let mut decoder = LineDecoder::new();
        assert_eq!(lines(&mut decoder, b"PW"), NONE);
        decoder.reset();
        assert_eq!(lines(&mut decoder, b"MV43\r"), ["MV43"]);
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
