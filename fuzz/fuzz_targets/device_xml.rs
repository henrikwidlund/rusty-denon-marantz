//! The device information and the device description.
#![no_main]

use denon_marantz_proto::{DeviceDescription, DeviceInfo};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(info) = DeviceInfo::parse(data) {
        let _ = info.is_av_receiver();
        let _ = info.is_avr_x();
        let _ = info.into_owned();
    }
    if let Ok(description) = DeviceDescription::parse(data) {
        let _ = description.brand();
        let _ = description.into_owned();
    }
});
