//! Text lines straight into the parser and the state. Names, levels and the
//! quick select must stay in their documented ranges. A source is never empty
//! and has no spaces at the end.
#![no_main]

mod checks;

use denon_marantz_proto::telnet::Message;
use denon_marantz_proto::{ChannelLevel, MAX_QUICK_SELECT, State, ToneLevel, Zone};
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
    let _ = state.main_playing_kind();
    let signal = &state.device().signal;
    let _ = signal.effective_pixel_depth();
    let _ = signal.video_color().input.map(|color| color.to_string());
    check_ranges(&state);
    checks::check_sources(&state);
});

fn check_level(level: ChannelLevel) {
    let range = ChannelLevel::MIN.db()..=ChannelLevel::MAX.db();
    assert!(range.contains(&level.db()), "{level:?}");
}

fn check_ranges(state: &State) {
    let device = state.device();
    // A name that is missing or empty is `None`.
    assert!(device.quick_select_names.len() <= usize::from(MAX_QUICK_SELECT));
    for names in [&device.quick_select_names, &device.network_preset_names] {
        assert!(names.iter().flatten().all(|name| !name.is_empty()));
    }
    let sound = &device.sound_settings;
    device
        .speaker_levels
        .iter()
        .chain(&device.channel_volumes)
        .map(|(_, level)| *level)
        .chain(sound.subwoofer_levels.into_iter().flatten())
        .chain(sound.center_level)
        .chain(sound.tactile_level)
        .chain(sound.tactile_channel_level)
        .for_each(check_level);
    for zone in [Zone::Main, Zone::Zone2, Zone::Zone3] {
        let zone = state.zone(zone);
        assert!(zone.quick_select.is_none_or(|n| n <= MAX_QUICK_SELECT));
        zone.left_level
            .into_iter()
            .chain(zone.right_level)
            .for_each(check_level);
        for tone in zone.bass.into_iter().chain(zone.treble) {
            assert!((ToneLevel::MIN..=ToneLevel::MAX).contains(&tone));
        }
    }
}
