//! Datagrams of the device search and locations.
#![no_main]

use denon_marantz_discovery::ssdp::{Location, Response};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(response) = Response::parse(data) {
        let _ = format!("{response:?}");
    }
    if let Ok(text) = std::str::from_utf8(data)
        && let Some(location) = Location::parse(text)
    {
        let _ = location.ip();
    }
});
