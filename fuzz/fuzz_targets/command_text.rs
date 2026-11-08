//! Commands with text from a caller: the line must have one end only.
#![no_main]

use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{Brand, Zone};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    for command in [
        Command::SelectSource(text),
        Command::SelectSoundMode(text),
        Command::Raw(text),
    ] {
        for brand in [Brand::Denon, Brand::Marantz] {
            for zone in [Zone::Main, Zone::Zone2, Zone::Zone3] {
                let mut out = String::new();
                if command.write_line(brand, zone, &mut out).is_ok() {
                    assert!(out.ends_with('\r'));
                    assert_eq!(out.matches(['\r', '\n']).count(), 1, "{out:?}");
                } else {
                    assert!(out.is_empty());
                }
            }
        }
    }
});
