//! Bytes from a telnet connection in chunks: decoder, parser, filter and state.
//! The lines must be the same as when the bytes come in one piece.
#![no_main]

mod chunks;

use denon_marantz_proto::State;
use denon_marantz_proto::telnet::{DuplicateFilter, LineDecoder, Message};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((sizes, rest)) = chunks::split_sizes(data) else {
        return;
    };
    let mut whole = Vec::new();
    LineDecoder::new().decode(rest, |line| whole.push(line.to_owned()));

    let mut decoder = LineDecoder::new();
    let mut filter = DuplicateFilter::new();
    let mut state = State::new(data[0] & 0x80 != 0);
    let mut count = 0;
    for part in chunks::chunks(sizes, rest) {
        decoder.decode(part, |line| {
            assert_eq!(whole.get(count).map(String::as_str), Some(line));
            count += 1;
            if let Some(message) = Message::parse(line)
                && filter.accept(message.parameter)
            {
                state.apply_message(&message);
            }
        });
    }
    assert_eq!(count, whole.len());
});
