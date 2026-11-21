//! Commands with text from a caller: the line must have one end only. The
//! settings parts and the query and spec lookups of each line must not panic.
#![no_main]

use denon_marantz_proto::appcommand::settings_parts_of_command;
use denon_marantz_proto::telnet::{Command, query_for_command};
use denon_marantz_proto::{Brand, Zone, spec_feature_of_line};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = settings_parts_of_command(text);
    let _ = query_for_command(text);
    let _ = spec_feature_of_line(text);
    for command in [
        Command::SelectSource(text),
        Command::SelectSoundMode(text),
        Command::VideoSelectSource(text),
        Command::RecordSource(text),
        Command::Raw(text),
    ] {
        for brand in [Brand::Denon, Brand::Marantz] {
            for zone in [Zone::Main, Zone::Zone2, Zone::Zone3] {
                let mut out = String::new();
                if command.write_line(brand, zone, &mut out).is_ok() {
                    assert!(out.ends_with('\r'));
                    assert_eq!(out.matches(['\r', '\n']).count(), 1, "{out:?}");
                    let _ = settings_parts_of_command(&out);
                    let _ = query_for_command(&out);
                    let _ = spec_feature_of_line(&out);
                } else {
                    assert!(out.is_empty());
                }
            }
        }
    }
});
