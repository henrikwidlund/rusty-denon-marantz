//! Text lines straight into the parser and the state.
#![no_main]

use denon_marantz_proto::State;
use denon_marantz_proto::telnet::Message;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else {
        return;
    };
    let text = String::from_utf8_lossy(rest);
    let mut state = State::new(first & 1 != 0);
    for line in text.split(['\r', '\n']) {
        if let Some(message) = Message::parse(line) {
            state.apply_message(&message);
        }
    }
    // The state must stay usable: read it back.
    let _ = state.clone();
    let signal = &state.device().signal;
    let _ = signal.effective_pixel_depth();
    let _ = signal.video_color().input.map(|color| color.to_string());
});
