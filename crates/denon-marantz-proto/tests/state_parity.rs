//! Compares the values that `State` takes from `AppCommand` answers with the
//! values that the Python converters give for the same text.
//!
//! The text comes from answers that real receivers sent
//! (`data/state_golden.tsv`).

use std::fs;
use std::path::PathBuf;

use denon_marantz_proto::appcommand::Field;
use denon_marantz_proto::{State, Zone};

const NONE: &str = "<none>";

fn unescape(text: &str) -> Option<String> {
    if text == NONE {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(match chars.next() {
                Some('t') => '\t',
                Some('n') => '\n',
                Some('r') => '\r',
                Some(other) => other,
                None => '\\',
            });
        } else {
            out.push(c);
        }
    }
    Some(out)
}

fn field(name: &str) -> Field {
    match name {
        "Muted" => Field::Muted,
        "Power" => Field::Power,
        "InputFunc" => Field::InputFunc,
        "Volume" => Field::Volume,
        "MaxVolume" => Field::MaxVolume,
        "AutoStandby" => Field::AutoStandby,
        "SoundModeRaw" => Field::SoundModeRaw,
        "Dimmer" => Field::Dimmer,
        "EcoMode" => Field::EcoMode,
        other => panic!("unknown field {other}"),
    }
}

/// The value in `State` as text, like the Python value.
fn rendered(state: &State, name: &str) -> Option<String> {
    let zone = state.zone(Zone::Main);
    match name {
        "Muted" => zone
            .muted
            .map(|m| if m { "True" } else { "False" }.to_owned()),
        "Power" => zone.power.map(|p| p.code().to_owned()),
        "InputFunc" => zone.source.clone(),
        "Volume" => zone.volume.map(|v| v.db().to_string()),
        "MaxVolume" => zone.max_volume.map(|v| v.db().to_string()),
        "AutoStandby" => zone.auto_standby.map(|v| v.label().to_owned()),
        "SoundModeRaw" => zone.sound_mode.clone(),
        "Dimmer" => state.device().dimmer.map(|v| v.label().to_owned()),
        "EcoMode" => state.device().eco_mode.map(|v| v.label().to_owned()),
        other => panic!("unknown field {other}"),
    }
}

/// Numbers are compared as numbers, because `-80.0` and `-80` are the same.
fn same(name: &str, actual: Option<&str>, expected: Option<&str>) -> bool {
    if matches!(name, "Volume" | "MaxVolume") {
        let number = |text: Option<&str>| text.map(|t| t.parse::<f64>().unwrap());
        return number(actual) == number(expected);
    }
    actual == expected
}

#[test]
fn answer_values_match_python() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/data/state_golden.tsv");
    let golden = fs::read_to_string(path).unwrap();
    let mut rows = 0;
    for row in golden.lines() {
        let f: Vec<&str> = row.split('\t').collect();
        let (name, raw, python) = (f[0], unescape(f[1]), f[2]);
        let expected = unescape(python).filter(|_| python != "<error>");

        let mut state = State::new(false);
        state.apply_field(Zone::Main, field(name), raw.as_deref());
        let actual = rendered(&state, name);

        // Python keeps any text as the power. State only knows the three
        // power values, and accepts lower case. Other text is unknown.
        let expected = if name == "Power" {
            expected
                .map(|text| text.to_ascii_uppercase())
                .filter(|text| matches!(text.as_str(), "ON" | "OFF" | "STANDBY"))
        } else {
            expected
        };
        assert!(
            same(name, actual.as_deref(), expected.as_deref()),
            "{name} {raw:?}: got {actual:?}, Python {expected:?}"
        );
        rows += 1;
    }
    assert!(rows > 50, "{rows}");
}

#[test]
fn real_update_answer_fills_the_state() {
    use denon_marantz_proto::appcommand::{self as ac, Response};
    use denon_marantz_proto::{AutoStandby, DimmerMode, EcoMode, Power};

    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/xml/AVC-X3700H-AppCommand-update-8080.xml");
    let body = fs::read(path).unwrap();
    // The first five commands of this answer, as the receiver was asked.
    let commands = [
        ac::GET_ALL_ZONE_POWER_STATUS,
        ac::GET_AUTO_STANDBY,
        ac::GET_DIMMER,
        ac::GET_ECO,
        ac::GET_ALL_ZONE_SOURCE,
    ];
    let response = Response::parse(&body, 11).unwrap();
    let mut state = State::new(false);
    for zone in Zone::ALL {
        response.extract(&commands, zone, |field, value| {
            state.apply_field(zone, field, value);
        });
    }
    assert_eq!(state.zone(Zone::Main).power, Some(Power::Off));
    assert_eq!(
        state.zone(Zone::Main).auto_standby,
        Some(AutoStandby::Minutes60)
    );
    assert_eq!(state.zone(Zone::Zone2).auto_standby, Some(AutoStandby::Off));
    assert_eq!(state.zone(Zone::Main).source.as_deref(), Some("MPLAY"));
    assert_eq!(state.zone(Zone::Zone2).source.as_deref(), Some("SOURCE"));
    assert_eq!(state.device().dimmer, Some(DimmerMode::Off));
    assert_eq!(state.device().eco_mode, Some(EcoMode::Auto));
}
