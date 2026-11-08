//! Bytes from a telnet connection in chunks: decoder, parser, filter and state.
#![no_main]

use denon_marantz_proto::State;
use denon_marantz_proto::telnet::{DuplicateFilter, LineDecoder, Message};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else {
        return;
    };
    // The first byte sets the size of the chunks, from 1 to 64 bytes.
    let chunk = usize::from(first % 64) + 1;
    let mut decoder = LineDecoder::new();
    let mut filter = DuplicateFilter::new();
    let mut state = State::new(first & 0x80 != 0);
    for part in rest.chunks(chunk) {
        decoder.decode(part, |line| {
            if let Some(message) = Message::parse(line) {
                if filter.accept(message.parameter) {
                    state.apply_message(&message);
                }
            }
        });
    }
});
