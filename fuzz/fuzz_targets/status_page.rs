//! The XML status pages of a zone.
#![no_main]

use denon_marantz_proto::status_page::{Page, extract, friendly_name, sources};
use denon_marantz_proto::{State, Zone};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The input is two pages, split at the byte in the middle.
    let (first, second) = data.split_at(data.len() / 2);
    let status = Page::parse(first).ok();
    let zone_page = Page::parse(second).ok();
    if let Some(page) = &status {
        let _ = friendly_name(page);
    }
    let mut state = State::new(false);
    for zone in [Zone::Main, Zone::Zone2, Zone::Zone3] {
        extract(
            status.as_ref(),
            zone_page.as_ref(),
            zone,
            &mut |field, value| {
                state.apply_field(zone, field, value);
            },
        );
    }
    sources(
        status.as_ref(),
        zone_page.as_ref(),
        |name, rename, in_use| {
            state.apply_renamed_source(name, rename);
            if let Some(in_use) = in_use {
                state.apply_source_in_use(name, in_use);
            }
        },
    );
});
