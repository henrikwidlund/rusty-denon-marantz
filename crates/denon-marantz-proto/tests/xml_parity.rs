//! Compares XML parsing with the results of the Python library on answers
//! that real receivers sent.
//!
//! `data/xml` has the answers. `data/appcommand_golden.tsv` and
//! `data/deviceinfo_golden.tsv` have the values that Python found.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use denon_marantz_proto::appcommand::{self as ac, Definition, Response};
use denon_marantz_proto::{DeviceInfo, Error, Zone};

const NONE: &str = "<none>";

const DEFINITIONS: [(&str, Definition); 10] = [
    ("GetAllZoneMuteStatus", ac::GET_ALL_ZONE_MUTE_STATUS),
    ("GetAllZonePowerStatus", ac::GET_ALL_ZONE_POWER_STATUS),
    ("GetAllZoneSource", ac::GET_ALL_ZONE_SOURCE),
    ("GetAllZoneVolume", ac::GET_ALL_ZONE_VOLUME),
    ("GetAutoStandby", ac::GET_AUTO_STANDBY),
    ("GetDimmer", ac::GET_DIMMER),
    ("GetECO", ac::GET_ECO),
    ("GetSurroundModeStatus", ac::GET_SURROUND_MODE_STATUS),
    ("GetToneControl", ac::GET_TONE_CONTROL),
    ("GetAudyssey", ac::GET_AUDYSSEY),
];

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

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

type Values = Vec<(String, Option<String>)>;

#[test]
fn appcommand_matches_python() {
    let golden = fs::read_to_string(data("appcommand_golden.tsv")).unwrap();
    let mut files = Vec::new();
    let mut expected: BTreeMap<(String, usize, String, String), Values> = BTreeMap::new();
    for row in golden.lines() {
        let f: Vec<&str> = row.split('\t').collect();
        match f[0] {
            "F" => files.push((
                f[1].to_owned(),
                f[2].to_owned(),
                f.get(3).map(|n| n.parse::<usize>().unwrap()),
            )),
            "R" => expected
                .entry((
                    f[1].to_owned(),
                    f[2].parse().unwrap(),
                    f[3].to_owned(),
                    f[4].to_owned(),
                ))
                .or_default()
                .push((f[5].to_owned(), unescape(f[6]))),
            other => panic!("unknown row {other}"),
        }
    }

    let (mut ok_files, mut html_files, mut compared) = (0, 0, 0);
    for (name, kind, count) in &files {
        let body = fs::read(data(&format!("xml/{name}"))).unwrap();
        match kind.as_str() {
            "html" => {
                assert_eq!(
                    Response::parse(&body, 0).unwrap_err(),
                    Error::HtmlResponse,
                    "{name}"
                );
                html_files += 1;
            }
            "ok" => {
                let count = count.unwrap();
                let response =
                    Response::parse(&body, count).unwrap_or_else(|e| panic!("{name}: {e}"));
                assert_eq!(response.items().count(), count, "{name}");
                for (i, item) in response.items().enumerate() {
                    for (dname, definition) in &DEFINITIONS {
                        for zone in Zone::ALL {
                            let mut got: Values = Vec::new();
                            definition.extract(&item, zone, &mut |field, value| {
                                got.push((format!("{field:?}"), value.map(str::to_owned)));
                            });
                            got.sort();
                            let key =
                                (name.clone(), i, (*dname).to_owned(), zone.name().to_owned());
                            let mut want = expected.remove(&key).unwrap_or_default();
                            want.sort();
                            assert_eq!(got, want, "{name} item {i} {dname} {zone:?}");
                            compared += want.len();
                        }
                    }
                }
                ok_files += 1;
            }
            other => panic!("{name}: {other}"),
        }
    }
    assert!(
        expected.is_empty(),
        "golden rows without a Rust result: {}",
        expected.len()
    );
    assert!(
        ok_files > 80 && html_files > 30 && compared > 1000,
        "{ok_files} {html_files} {compared}"
    );
}

#[test]
fn wrong_item_count_is_an_error() {
    let body = fs::read(data("xml/AVC-X3700H-AppCommand-update-8080.xml")).unwrap();
    let count = Response::parse(&body, usize::MAX).unwrap_err();
    assert!(
        matches!(
            count,
            Error::UnexpectedItemCount {
                expected: usize::MAX,
                ..
            }
        ),
        "{count:?}"
    );
    assert!(Response::parse(&body, 0).is_err());
}

#[test]
fn deviceinfo_matches_python() {
    let golden = fs::read_to_string(data("deviceinfo_golden.tsv")).unwrap();
    let (mut ok, mut html) = (0, 0);
    for row in golden.lines() {
        let f: Vec<&str> = row.split('\t').collect();
        let name = f[0];
        let body = fs::read(data(&format!("xml/{name}"))).unwrap();
        if f[1] == "html" {
            assert_eq!(
                DeviceInfo::parse(&body).unwrap_err(),
                Error::HtmlResponse,
                "{name}"
            );
            html += 1;
            continue;
        }
        assert_eq!(f[1], "ok", "{name}");
        let info = DeviceInfo::parse(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            info.comm_api_version.as_deref(),
            unescape(f[2]).as_deref(),
            "{name}"
        );
        assert_eq!(
            info.model_name.as_deref(),
            unescape(f[3]).as_deref(),
            "{name}"
        );
        assert_eq!(
            info.device_zones.map(|z| z.to_string()),
            unescape(f[4]),
            "{name}"
        );
        assert_eq!(info.dirac_live, f[5] == "True", "{name}");
        assert_eq!(info.is_avr_x(), f[6] == "True", "{name}");
        ok += 1;
    }
    assert!(ok > 15 && html > 3, "{ok} {html}");
}
