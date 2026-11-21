//! The device information and the device description. The settings request
//! from the listed queries must never fail.
#![no_main]

use denon_marantz_proto::appcommand::{SettingsPart, write_settings_request};
use denon_marantz_proto::{DeviceDescription, DeviceInfo, is_old_model_page};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(info) = DeviceInfo::parse(data) {
        for range in info.ranges.all() {
            assert!(range.min_tenths <= range.max_tenths, "{range:?}");
            assert_ne!(range.step_tenths, Some(0), "{range:?}");
            assert!(range.contains_tenths(range.min_tenths), "{range:?}");
            let _ = range.contains_tenths(range.max_tenths);
            let _ = range.contains_whole(range.max_tenths / 10);
        }
        if let Some(features) = info.features {
            for line in data.split(|byte| *byte == b'\n') {
                let _ = features.allows(std::str::from_utf8(line).unwrap_or(""));
            }
        }
        if let Some(listed) = &info.queries {
            for zones in 1..=3 {
                for part in [SettingsPart::Fast, SettingsPart::Slow] {
                    let mut out = Vec::new();
                    let count =
                        write_settings_request(listed, zones, info.features, part, &mut out)
                            .expect("built-in names are valid");
                    assert_eq!(count == 0, out.is_empty());
                }
            }
        }
        let _ = info.is_av_receiver();
        let _ = info.is_avr_x();
        let _ = info.into_owned();
    } else {
        let _ = is_old_model_page(data);
    }
    if let Ok(description) = DeviceDescription::parse(data) {
        let _ = description.brand();
        let _ = description.into_owned();
    }
});
