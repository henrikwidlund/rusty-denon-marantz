//! Checks XML parsing on answers that real receivers sent.
//!
//! `data/xml` has the answers. `data/appcommand_golden.tsv` and
//! `data/deviceinfo_golden.tsv` have the expected values.

mod common;

use common::{read_bytes, read_data, unescape};

use std::collections::BTreeMap;

use denon_marantz_proto::appcommand::{self as ac, Definition, Response};
use denon_marantz_proto::{
    Brand, DeviceDescription, DeviceInfo, Error, NowPlaying, TunerBand, TunerInfo, TunerPage, Zone,
};

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

type Values = Vec<(String, Option<String>)>;

#[test]
fn appcommand_answers_have_the_expected_values() {
    let golden = read_data("appcommand_golden.tsv");
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
        let body = read_bytes(&format!("xml/{name}"));
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
                            // These commands give nothing for a blank value.
                            if !definition.blank_is_unknown {
                                want.retain(|(_, value)| value.is_some());
                            }
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
        ok_files > 80 && html_files > 30 && compared > 700,
        "{ok_files} {html_files} {compared}"
    );
}

#[test]
fn wrong_item_count_is_an_error() {
    let body = read_bytes("xml/AVC-X3700H-AppCommand-update-8080.xml");
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
fn device_info_has_the_expected_values() {
    let golden = read_data("deviceinfo_golden.tsv");
    let (mut ok, mut html) = (0, 0);
    for row in golden.lines() {
        let f: Vec<&str> = row.split('\t').collect();
        let name = f[0];
        let body = read_bytes(&format!("xml/{name}"));
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

#[test]
fn device_info_tells_the_tuner() {
    let bands = |bands: &[TunerBand]| TunerInfo::Present(bands.iter().copied().collect());
    let (am, fm, dab) = (TunerBand::Am, TunerBand::Fm, TunerBand::Dab);
    let cases = [
        // Older receivers describe the tuner and its bands.
        ("AVR-X1100W-Deviceinfo.xml", bands(&[am, fm])),
        ("AVR-X4000-Deviceinfo.xml", bands(&[fm])),
        ("AV7703-Deviceinfo-8080.xml", bands(&[am, fm])),
        ("M-RC610-Deviceinfo.xml", bands(&[fm, dab])),
        // `Control` 0: no tuner.
        ("M-CR510-Deviceinfo.xml", TunerInfo::Absent),
        ("DRA-N4-Deviceinfo.xml", TunerInfo::Absent),
        // Newer receivers only link the tuner source to it.
        ("AVC-A10H-Deviceinfo-8080.xml", bands(&[])),
        ("AVC-X3700H-Deviceinfo-8080.xml", bands(&[])),
    ];
    for (name, expected) in cases {
        let body = read_bytes(&format!("xml/{name}"));
        let info = DeviceInfo::parse(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(info.tuner, expected, "{name}");
    }
}

#[test]
fn device_info_tells_the_quick_select_count() {
    let cases = [
        ("AVC-A10H-Deviceinfo-8080.xml", Some(4)),
        ("AV7703-Deviceinfo-8080.xml", Some(4)),
        ("AVR-X1100W-Deviceinfo.xml", Some(4)),
        // `Control` 0: no quick select.
        ("DRA-N4-Deviceinfo.xml", Some(0)),
        // The file does not say.
        ("NR1604-Deviceinfo.xml", None),
    ];
    for (name, expected) in cases {
        let body = read_bytes(&format!("xml/{name}"));
        let info = DeviceInfo::parse(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(info.quick_select_count, expected, "{name}");
    }
}

#[test]
fn device_description_of_a_real_receiver() {
    let body = read_bytes("xml/AVR-X1600H-description.xml");
    let description = DeviceDescription::parse(&body).unwrap();
    assert_eq!(description.manufacturer, "Denon");
    assert_eq!(description.model_name.as_deref(), Some("Denon AVR-X1600H"));
    assert_eq!(
        description.friendly_name.as_deref(),
        Some("Denon AVR-X1600H")
    );
    assert_eq!(description.serial_number.as_deref(), Some("XXX"));
    assert_eq!(description.presentation_url, None);
    assert_eq!(description.brand(), Brand::Denon);
}

#[test]
fn tone_control_answers_when_it_is_off_and_on() {
    let values = |name: &str| {
        let body = read_bytes(&format!("xml/{name}"));
        let response = Response::parse(&body, 1).unwrap();
        let item = response.items().next().unwrap();
        let mut got: Values = Vec::new();
        ac::GET_TONE_CONTROL.extract(&item, Zone::Main, &mut |field, value| {
            got.push((format!("{field:?}"), value.map(str::to_owned)));
        });
        got
    };
    let get = |got: &Values, field: &str| {
        got.iter()
            .find(|(name, _)| name == field)
            .and_then(|(_, value)| value.clone())
    };
    let off = values("AVR-X1700H-AppCommand-tonecontrol-dormant.xml");
    assert_eq!(get(&off, "ToneControlStatus").as_deref(), Some("0"));
    assert_eq!(get(&off, "ToneControlAdjust"), None);
    assert_eq!(get(&off, "BassLevel"), None);
    let on = values("AVR-X1700H-AppCommand-tonecontrol-engaged.xml");
    assert_eq!(get(&on, "ToneControlStatus").as_deref(), Some("1"));
    assert_eq!(get(&on, "ToneControlAdjust").as_deref(), Some("1"));
    assert_eq!(get(&on, "BassLevel").as_deref(), Some("+4dB"));
    assert_eq!(get(&on, "TrebleLevel").as_deref(), Some("-4dB"));
}

/// The names of the files in `data/xml` that end with `suffix`.
fn fixtures(suffix: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(common::data("xml"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .filter(|name| name.ends_with(suffix))
        .collect();
    names.sort();
    names
}

#[test]
fn now_playing_pages_of_all_receivers_are_read() {
    let (mut pages, mut playing, mut idle) = (0, 0, 0);
    for name in fixtures("formNetAudio_StatusXml.xml") {
        let body = read_bytes(&format!("xml/{name}"));
        let now = NowPlaying::parse_net_audio(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
        match now.is_playing() {
            Some(true) => {
                assert!(now.title.is_some(), "{name}");
                playing += 1;
            }
            // A page without a first line is idle too.
            Some(false) | None => idle += 1,
        }
        pages += 1;
    }
    // Pages that show a menu, a list or nothing do not play anything.
    assert!(
        pages >= 22 && playing >= 6 && idle >= 10,
        "{pages} {playing} {idle}"
    );

    let mut hd_pages = 0;
    for name in fixtures("formTuner_HdXml.xml") {
        let body = read_bytes(&format!("xml/{name}"));
        NowPlaying::parse_hd_radio(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
        hd_pages += 1;
    }
    assert!(hd_pages >= 14, "{hd_pages}");

    let mut tuned = 0;
    let names = fixtures("formTuner_TunerXml.xml");
    for name in &names {
        let body = read_bytes(&format!("xml/{name}"));
        let page = TunerPage::parse(&body).unwrap_or_else(|e| panic!("{name}: {e}"));
        tuned += usize::from(page.frequency.is_some());
    }
    // Only the page of a DAB receiver is tuned.
    assert!(names.len() >= 15 && tuned == 1, "{} {tuned}", names.len());
}
