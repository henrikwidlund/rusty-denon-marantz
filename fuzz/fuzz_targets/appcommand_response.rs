//! XML answers of the `AppCommand` endpoints. A source is never empty and has
//! no spaces at the end.
#![no_main]

mod checks;

use denon_marantz_proto::appcommand::{
    GET_ACTIVE_SPEAKER, GET_ALL_ZONE_MUTE_STATUS, GET_ALL_ZONE_POWER_STATUS, GET_ALL_ZONE_SOURCE,
    GET_ALL_ZONE_VOLUME, GET_AUDIO_INFO, GET_AUDYSSEY, GET_AUTO_STANDBY, GET_DIMMER, GET_ECO,
    GET_FRIENDLY_NAME, GET_INPUT_SIGNAL, GET_OUTPUT_SETTINGS, GET_SURROUND_MODE_STATUS,
    GET_TONE_CONTROL, GET_VIDEO_INFO, GET_ZONE_NAME, Response, settings_lines,
};
use denon_marantz_proto::telnet::Message;
use denon_marantz_proto::{ChannelLayout, State, Zone};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&first, body)) = data.split_first() else {
        return;
    };
    let definitions = [
        GET_ALL_ZONE_MUTE_STATUS,
        GET_ALL_ZONE_POWER_STATUS,
        GET_ALL_ZONE_SOURCE,
        GET_ALL_ZONE_VOLUME,
        GET_AUDYSSEY,
        GET_AUTO_STANDBY,
        GET_DIMMER,
        GET_ECO,
        GET_FRIENDLY_NAME,
        GET_SURROUND_MODE_STATUS,
        GET_TONE_CONTROL,
        GET_ZONE_NAME,
        GET_VIDEO_INFO,
        GET_AUDIO_INFO,
        GET_INPUT_SIGNAL,
        GET_ACTIVE_SPEAKER,
        GET_OUTPUT_SETTINGS,
    ];
    // The first byte sets how many commands the answer must have.
    let expected = usize::from(first) % (definitions.len() + 1);
    if let Ok(response) = Response::parse_at_most(body, expected) {
        for (index, item) in response.matched(&definitions[..expected]) {
            assert!(index < expected);
            let _ = item.name();
        }
        // Each settings line must parse as a telnet event. Applying it must
        // not panic.
        let mut state = State::new(false);
        settings_lines(&response, |line, _| {
            let message = Message::parse(line).expect("settings line is an event");
            let _ = state.apply_message(&message);
        });
        checks::check_sources(&state);
    }
    let Ok(response) = Response::parse(body, expected) else {
        return;
    };
    let mut state = State::new(false);
    for zone in [Zone::Main, Zone::Zone2, Zone::Zone3] {
        response.extract(&definitions[..expected], zone, |field, value| {
            state.apply_field(zone, field, value);
        });
    }
    for item in response.items() {
        let _ = item.is_error();
        let _ = item.error_code();
        item.renamed_sources(|name, rename| {
            state.apply_renamed_source(name, rename);
        });
        item.source_use(|name, value| {
            state.apply_source_use(name, value);
        });
        let _ = state.apply_channel_grid(true, ChannelLayout::from_appcommand(&item));
        item.params(|name, control, text| {
            let _ = (name.len(), control, text.len());
        });
    }
    checks::check_sources(&state);
});
