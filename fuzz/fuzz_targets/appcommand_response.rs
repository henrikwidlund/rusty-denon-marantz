//! XML answers of the `AppCommand` endpoints.
#![no_main]

use denon_marantz_proto::appcommand::{
    GET_ALL_ZONE_MUTE_STATUS, GET_ALL_ZONE_POWER_STATUS, GET_ALL_ZONE_SOURCE,
    GET_ALL_ZONE_VOLUME, GET_AUDYSSEY, GET_AUTO_STANDBY, GET_DIMMER, GET_ECO,
    GET_FRIENDLY_NAME, GET_SURROUND_MODE_STATUS, GET_TONE_CONTROL, GET_ZONE_NAME, Response,
};
use denon_marantz_proto::{State, Zone};
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
    ];
    // The first byte sets how many commands the answer must have.
    let expected = usize::from(first) % (definitions.len() + 1);
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
    }
});
