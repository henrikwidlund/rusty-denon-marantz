//! Message fields, value decoding and command text with any input.
#![no_main]

use denon_marantz_heos_proto::{Command, Event, Message, PlayerId, decode};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let message = Message::new(text);
    for (key, value) in message.fields() {
        let _ = message.get(key);
        if let Some(value) = value {
            let _ = decode(value);
        }
    }
    for name in [
        "player_state_changed",
        "player_now_playing_progress",
        "player_playback_error",
        "repeat_mode_changed",
        "shuffle_mode_changed",
        "user_changed",
    ] {
        let _ = Event::parse(name, message);
    }

    // A command line has no line break inside, and the value comes back.
    let pid = PlayerId(1);
    let mut line = String::new();
    if (Command::PlayStream { pid, url: text })
        .write(&mut line)
        .is_ok()
    {
        let body = line.strip_suffix("\r\n").unwrap();
        assert!(!body.contains(['\r', '\n']));
        let url = body.split_once("&url=").unwrap().1;
        assert_eq!(decode(url), text);
    }
});
