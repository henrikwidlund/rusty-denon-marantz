//! The status pages and the events about what plays.
#![no_main]

use denon_marantz_proto::telnet::Message;
use denon_marantz_proto::{NowPlaying, State, TunerPage};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = NowPlaying::parse_net_audio(data);
    let _ = NowPlaying::parse_hd_radio(data);
    let _ = TunerPage::parse(data);
    if let Ok(text) = std::str::from_utf8(data) {
        let mut state = State::new(false);
        for line in text.lines().take(64) {
            if let Some(message) = Message::parse(line) {
                let _ = state.apply_message(&message);
            }
        }
    }
});
