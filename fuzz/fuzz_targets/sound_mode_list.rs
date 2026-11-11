//! Lines of the sound mode list through the telnet parser and the state, then the codes.
#![no_main]

use denon_marantz_proto::telnet::Message;
use denon_marantz_proto::{SoundModeCategory, State};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let text = String::from_utf8_lossy(data);
    let mut state = State::new(false);
    for line in text.split(['\r', '\n']) {
        let line = if line.starts_with("OP") {
            line.to_owned()
        } else {
            format!("OPSMLALL {line}")
        };
        if let Some(message) = Message::parse(&line) {
            state.apply_message(&message);
        }
    }
    let list = &state.device().sound_modes;
    for entry in list.as_slice() {
        let _ = (entry.group(), entry.code());
    }
    let _ = list.in_category(SoundModeCategory::Movie).count();
});
