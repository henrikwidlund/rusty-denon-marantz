//! Lines of the sound mode list through the telnet parser and the state, then the codes
//! and the entry for each line as a reported mode. Each name finds an entry with the
//! same words, and the categories split the list.
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
        let found = list
            .entry_for(&entry.name)
            .expect("a listed name has an entry");
        assert!(same_words(&found.name, &entry.name), "{found:?} {entry:?}");
    }
    let total: usize = SoundModeCategory::ALL
        .iter()
        .map(|&category| list.in_category(category).count())
        .sum();
    assert_eq!(total, list.as_slice().len());
    for line in text.split(['\r', '\n']) {
        let _ = list.entry_for(line);
    }
});

/// Compares words without ASCII case.
fn same_words(a: &str, b: &str) -> bool {
    let mut a = a.split_ascii_whitespace();
    let mut b = b.split_ascii_whitespace();
    loop {
        match (a.next(), b.next()) {
            (Some(x), Some(y)) if x.eq_ignore_ascii_case(y) => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}
