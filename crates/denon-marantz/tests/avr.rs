//! `Avr` against fake receivers.

#![cfg(feature = "http")]

mod common;

use common::{
    AUDIO_INFO_ONLY_INFO, AVR_X_INFO, AVR_X_OUTPUT_SETTINGS_INFO, Fake, Reply, UPDATE_ANSWER,
    appcommand0300_answer, appcommand0300_item, avr_x_receiver, builder, config,
    friendly_name_answer, is_friendly_name_request, output_settings_item, receiver_with_audyssey,
    receiver_with_info_and_signal_answers, receiver_with_signal_answers, web_interface_with_pages,
};
use denon_marantz::{Avr, Error, ListedQueries, PageStatus, TypeProof, UpdateMethod};
use denon_marantz_proto::{
    AutoStandby, Availability, Brand, DimmerMode, EcoMode, HdmiOutput, NetworkControl, Power,
    ReceiverType, SettingsGroup, ToneLevel, Volume, Zone,
};

const OLD_STATUS: &[u8] = include_bytes!(
    "../../denon-marantz-proto/tests/data/xml/AVR-3313-formMainZone_MainZoneXmlStatus.xml"
);
const OLD_ZONE_PAGE: &[u8] = include_bytes!(
    "../../denon-marantz-proto/tests/data/xml/AVR-3313-formMainZone_MainZoneXml.xml"
);
const OLD_ZONE2_STATUS: &[u8] = include_bytes!(
    "../../denon-marantz-proto/tests/data/xml/AVR-3313-formZone2_Zone2XmlStatus.xml"
);

/// Made up: no capture has readable XML without the AVR-X marks.
const OLD_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0100</CommApiVers><ModelName>AVR-FAKE</ModelName>\
    <DeviceZones>2</DeviceZones></Device_Info>";

/// An older receiver that has status pages and an error page for the rest.
async fn old_receiver() -> Fake {
    Fake::start(|seen| {
        let path = seen.path.split('?').next().unwrap_or_default();
        match path {
            "/goform/Deviceinfo.xml" => Reply::Body(OLD_INFO.to_vec()),
            "/goform/formMainZone_MainZoneXmlStatus.xml" => Reply::Body(OLD_STATUS.to_vec()),
            "/goform/formMainZone_MainZoneXml.xml" => Reply::Body(OLD_ZONE_PAGE.to_vec()),
            "/goform/formZone2_Zone2XmlStatus.xml" => Reply::Body(OLD_ZONE2_STATUS.to_vec()),
            "/goform/AppCommand.xml" => {
                Reply::Body(b"<html><body>Form is not defined</body></html>".to_vec())
            }
            _ => Reply::Status(404),
        }
    })
    .await
}

async fn connect(fake: &Fake, receiver_type: ReceiverType) -> Result<Avr, Error> {
    builder(fake, receiver_type).connect().await
}

#[tokio::test]
async fn connects_to_an_avr_x_receiver() {
    let fake = avr_x_receiver().await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    let info = avr.info();
    assert_eq!(info.receiver_type, ReceiverType::AvrX);
    assert_eq!(info.port, fake.port);
    assert_eq!(info.update_method, UpdateMethod::AppCommand);
    assert_eq!(info.friendly_name, "Living room");
    assert_eq!(info.brand, Brand::Denon);
    assert_eq!(info.manufacturer.as_deref(), Some("Denon"));
    assert_eq!(info.model_name.as_deref(), Some("Denon AVR-X1600H"));
    assert_eq!(info.serial_number.as_deref(), Some("XXX"));
    assert_eq!(info.comm_api_version.as_deref(), Some("0301"));
    assert_eq!(info.mac_address.as_deref(), Some("0123456789AB"));
    assert_eq!(info.type_proof, TypeProof::Read);
    assert_eq!(info.zones, [Zone::Main, Zone::Zone2]);
    // Nothing is read until update.
    assert_eq!(avr.snapshot().zone(Zone::Main).power, None);
}

#[tokio::test]
async fn update_reads_all_zones_with_one_request() {
    let fake = avr_x_receiver().await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    let before = fake.requests().len();
    avr.update().await.unwrap();
    let requests = fake.requests();
    assert_eq!(requests.len() - before, 1, "{requests:?}");
    let last = requests.last().unwrap();
    assert_eq!(
        (last.method.as_str(), last.path.as_str()),
        ("POST", "/goform/AppCommand.xml")
    );
    assert_eq!(
        String::from_utf8_lossy(&last.body)
            .matches("<cmd id=\"1\">")
            .count(),
        13
    );

    let state = avr.snapshot();
    let main = state.zone(Zone::Main);
    assert_eq!(main.power, Some(Power::On));
    assert_eq!(main.auto_standby, Some(AutoStandby::Minutes60));
    assert_eq!(main.source.as_deref(), Some("MPLAY"));
    assert_eq!(main.volume.map(Volume::db), Some(-36.5));
    assert_eq!(main.max_volume.map(Volume::db), Some(-10.0));
    assert_eq!(main.muted, Some(false));
    assert_eq!(main.sound_mode.as_deref(), Some("DOLBY ATMOS"));
    let zone2 = state.zone(Zone::Zone2);
    assert_eq!(zone2.power, Some(Power::Off));
    assert_eq!(zone2.auto_standby, Some(AutoStandby::Off));
    assert_eq!(zone2.source.as_deref(), Some("CD"));
    assert_eq!(zone2.volume, Some(Volume::MIN));
    assert_eq!(zone2.max_volume, None);
    assert_eq!(zone2.muted, Some(true));
    // Only the main zone has a sound mode.
    assert_eq!(zone2.sound_mode, None);
    assert_eq!(state.device().dimmer, Some(DimmerMode::Dim));
    assert_eq!(state.device().eco_mode, Some(EcoMode::On));
    // Zone 3 is not read: the receiver has two zones.
    assert_eq!(state.zone(Zone::Zone3).power, None);
}

#[tokio::test]
async fn watchers_are_told_only_about_changes() {
    let fake = avr_x_receiver().await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    let mut watcher = avr.state();
    assert!(!watcher.has_changed().unwrap());

    avr.update().await.unwrap();
    assert!(watcher.has_changed().unwrap());
    assert_eq!(
        watcher.borrow_and_update().zone(Zone::Main).power,
        Some(Power::On)
    );

    avr.update().await.unwrap();
    assert!(
        !watcher.has_changed().unwrap(),
        "same answer, no notification"
    );
}

#[tokio::test]
async fn connects_to_an_older_receiver_with_status_pages() {
    let fake = old_receiver().await;
    let avr = connect(&fake, ReceiverType::Avr).await.unwrap();
    let info = avr.info();
    assert_eq!(info.receiver_type, ReceiverType::Avr);
    assert_eq!(info.port, fake.port);
    assert_eq!(info.update_method, UpdateMethod::StatusPages);
    assert_eq!(info.friendly_name, "DENON AVR-3313");
    // There is no device description.
    assert_eq!(info.brand, Brand::Denon);
    assert_eq!(info.model_name, None);
    assert_eq!(info.zones, [Zone::Main, Zone::Zone2]);

    let before = fake.requests().len();
    avr.update().await.unwrap();
    // Two pages for each of the two zones.
    assert_eq!(fake.requests().len() - before, 4);
    let state = avr.snapshot();
    let main = state.zone(Zone::Main);
    assert_eq!(main.power, Some(Power::On));
    assert_eq!(main.volume.map(Volume::db), Some(-70.0));
    assert_eq!(main.muted, Some(false));
    assert_eq!(main.source.as_deref(), Some("SAT/CBL"));
    assert_eq!(main.sound_mode.as_deref(), Some("PLII CINEMA"));
    assert_eq!(state.zone(Zone::Zone2).power, Some(Power::Off));
    assert_eq!(state.zone(Zone::Zone2).volume.map(Volume::db), Some(-58.0));
    // The source lists of the status page.
    let sources = &state.device().sources;
    assert_eq!(sources.name("SAT/CBL"), Some("CBL/SAT"));
    assert_eq!(sources.name("MPLAY"), Some("MEDIA PLAYER"));
    assert_eq!(
        sources.get("BD").and_then(|source| source.in_use),
        Some(true)
    );
    assert_eq!(sources.as_slice().len(), 13);
}

#[tokio::test]
async fn zone_pages_use_the_zone_name() {
    let fake = old_receiver().await;
    let avr = connect(&fake, ReceiverType::Avr).await.unwrap();
    avr.update().await.unwrap();
    let paths: Vec<String> = fake.requests().into_iter().map(|seen| seen.path).collect();
    assert!(
        paths.contains(&"/goform/formMainZone_MainZoneXml.xml?ZoneName=Zone2".to_owned()),
        "{paths:?}"
    );
    assert!(
        paths.contains(&"/goform/formZone2_Zone2XmlStatus.xml".to_owned()),
        "{paths:?}"
    );
}

#[tokio::test]
async fn avr_x_without_appcommand_uses_status_pages() {
    let fake = Fake::start(|seen| {
        let path = seen.path.split('?').next().unwrap_or_default();
        match path {
            "/goform/Deviceinfo.xml" => Reply::Body(AVR_X_INFO.to_vec()),
            // `AppCommand.xml` is not found: the wildcard answers 404.
            "/goform/formMainZone_MainZoneXmlStatus.xml" => Reply::Body(OLD_STATUS.to_vec()),
            "/goform/formMainZone_MainZoneXml.xml" => Reply::Body(OLD_ZONE_PAGE.to_vec()),
            _ => Reply::Status(404),
        }
    })
    .await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    assert_eq!(avr.info().receiver_type, ReceiverType::AvrX);
    assert_eq!(avr.info().update_method, UpdateMethod::StatusPages);
    avr.update().await.unwrap();
    // The status page of zone 2 is missing, so its power comes from the other page.
    assert_eq!(avr.snapshot().zone(Zone::Main).power, Some(Power::On));
    assert_eq!(avr.snapshot().zone(Zone::Zone2).power, Some(Power::On));
}

#[tokio::test]
async fn single_zone_receiver_has_one_zone_and_a_default_name() {
    let fake = Fake::start(|seen| match seen.path.as_str() {
        "/goform/Deviceinfo.xml" => Reply::Body(
            b"<Device_Info><CommApiVers>0301</CommApiVers><DeviceZones>1</DeviceZones></Device_Info>".to_vec(),
        ),
        _ => Reply::Status(404),
    })
    .await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    assert_eq!(avr.info().zones, [Zone::Main]);
    // The default name has the brand.
    assert_eq!(avr.info().friendly_name, "Denon AVR");
}

#[tokio::test]
async fn unusable_description_is_not_an_error() {
    for description in [
        &b"<html><body>no</body></html>"[..],
        b"<root><device><manufacturer>Sonos</manufacturer></device></root>",
    ] {
        let fake = Fake::start(move |seen| match seen.path.as_str() {
            "/goform/Deviceinfo.xml" => Reply::Body(AVR_X_INFO.to_vec()),
            "/description.xml" => Reply::Body(description.to_vec()),
            "/goform/AppCommand.xml" => Reply::Body(friendly_name_answer("Den")),
            _ => Reply::Status(404),
        })
        .await;
        let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
        assert_eq!(avr.info().brand, Brand::Denon);
        assert_eq!(avr.info().model_name, None);
        assert_eq!(avr.info().friendly_name, "Den");
    }
}

#[tokio::test]
async fn marantz_description_gives_the_brand() {
    let marantz = b"<root xmlns=\"urn:schemas-upnp-org:device-1-0\"><device>\
        <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>\
        <manufacturer>Marantz</manufacturer><modelName>Marantz NR1711</modelName>\
        <friendlyName>Marantz Kitchen</friendlyName><serialNumber>S1</serialNumber></device></root>";
    let fake = Fake::start(|seen| match seen.path.as_str() {
        "/goform/Deviceinfo.xml" => Reply::Body(AVR_X_INFO.to_vec()),
        "/description.xml" => Reply::Body(marantz.to_vec()),
        "/goform/AppCommand.xml" => Reply::Body(b"<rx><cmd/></rx>".to_vec()),
        _ => Reply::Status(404),
    })
    .await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    assert_eq!(avr.info().brand, Brand::Marantz);
    assert_eq!(avr.info().model_name.as_deref(), Some("Marantz NR1711"));
    // The answer had no name, so the one from the description is used.
    assert_eq!(avr.info().friendly_name, "Marantz Kitchen");
}

#[tokio::test]
async fn given_brand_is_used_without_a_description() {
    let fake = Fake::start(|seen| match seen.path.as_str() {
        "/goform/Deviceinfo.xml" => Reply::Body(AVR_X_INFO.to_vec()),
        "/goform/AppCommand.xml" => Reply::Body(b"<rx><cmd/></rx>".to_vec()),
        _ => Reply::Status(404),
    })
    .await;
    let avr = builder(&fake, ReceiverType::AvrX)
        .brand(Brand::Marantz)
        .connect()
        .await
        .unwrap();
    assert_eq!(avr.info().brand, Brand::Marantz);
    assert_eq!(avr.info().friendly_name, "Marantz AVR");
}

#[tokio::test]
async fn description_brand_wins_over_the_given_one() {
    let fake = avr_x_receiver().await;
    let avr = builder(&fake, ReceiverType::AvrX)
        .brand(Brand::Marantz)
        .connect()
        .await
        .unwrap();
    assert_eq!(avr.info().brand, Brand::Denon);
}

#[tokio::test]
async fn description_is_tried_again_unless_it_timed_out() {
    for (hang, tries) in [(false, 2), (true, 1)] {
        let fake = Fake::start(move |seen| match seen.path.as_str() {
            "/goform/Deviceinfo.xml" => Reply::Body(AVR_X_INFO.to_vec()),
            "/description.xml" if hang => Reply::Hang,
            "/description.xml" => Reply::Close,
            _ => Reply::Status(404),
        })
        .await;
        connect(&fake, ReceiverType::AvrX).await.unwrap();
        let seen = fake
            .requests()
            .iter()
            .filter(|seen| seen.path == "/description.xml")
            .count();
        assert_eq!(seen, tries);
    }
}

#[tokio::test]
async fn nothing_answering_is_an_error() {
    let unused = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = unused.local_addr().unwrap().port();
    drop(unused);
    let error = Avr::builder("127.0.0.1")
        .http_config(config())
        .probes(&[(ReceiverType::AvrX, port)])
        .description_port(port)
        .connect()
        .await
        .unwrap_err();
    assert!(error.is_unreachable(), "{error:?}");
}

#[tokio::test]
async fn silence_from_the_description_is_not_an_error() {
    let fake = Fake::start(|seen| match seen.path.as_str() {
        "/goform/Deviceinfo.xml" => Reply::Body(AVR_X_INFO.to_vec()),
        "/description.xml" => Reply::Hang,
        _ => Reply::Status(404),
    })
    .await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    assert_eq!(avr.info().model_name, None);
}

#[tokio::test]
async fn bad_answers_to_update_are_errors() {
    let answers: [&[u8]; 4] = [
        b"<rx><cmd/></rx>",
        b"<html><body>Error</body></html>",
        b"<rx><cmd></rx>",
        b"nothing",
    ];
    for answer in answers {
        let fake = Fake::start(
            move |seen| match (seen.method.as_str(), seen.path.as_str()) {
                ("GET", "/goform/Deviceinfo.xml") => Reply::Body(AVR_X_INFO.to_vec()),
                ("POST", "/goform/AppCommand.xml") if is_friendly_name_request(&seen.body) => {
                    Reply::Body(friendly_name_answer("X"))
                }
                ("POST", _) => Reply::Body(answer.to_vec()),
                _ => Reply::Status(404),
            },
        )
        .await;
        let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
        let error = avr.update().await.unwrap_err();
        assert!(matches!(error, Error::Protocol { .. }), "{error:?}");
        assert!(error.is_invalid_response());
        assert_eq!(avr.snapshot().zone(Zone::Main).power, None);
    }
}

#[tokio::test]
async fn update_reports_a_receiver_that_stops_answering() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    let silent = Arc::new(AtomicBool::new(false));
    let flag = silent.clone();
    let fake = Fake::start(
        move |seen| match (seen.method.as_str(), seen.path.as_str()) {
            ("GET", "/goform/Deviceinfo.xml") => Reply::Body(AVR_X_INFO.to_vec()),
            ("POST", _) if is_friendly_name_request(&seen.body) => {
                Reply::Body(friendly_name_answer("X"))
            }
            ("POST", _) if flag.load(Ordering::SeqCst) => Reply::Hang,
            ("POST", _) => Reply::Body(UPDATE_ANSWER.to_vec()),
            _ => Reply::Status(404),
        },
    )
    .await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    avr.update().await.unwrap();
    silent.store(true, Ordering::SeqCst);
    let error = avr.update().await.unwrap_err();
    assert!(error.is_timeout(), "{error:?}");
    // The values from the first update stay.
    assert_eq!(avr.snapshot().zone(Zone::Main).power, Some(Power::On));
}

#[tokio::test]
async fn update_reads_the_source_names_and_the_hidden_sources() {
    let fake = avr_x_receiver().await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    avr.update().await.unwrap();
    let state = avr.snapshot();
    let sources = &state.device().sources;
    // The XML names are the telnet codes in the list.
    assert_eq!(sources.name("MPLAY"), Some("Apple TV"));
    assert_eq!(sources.get("MPLAY").unwrap().in_use, Some(true));
    assert_eq!(sources.name("CD"), None);
    assert_eq!(sources.get("CD").unwrap().in_use, Some(false));
}

#[tokio::test]
async fn update_reads_the_zone_names_and_the_friendly_name() {
    let fake = avr_x_receiver().await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    avr.update().await.unwrap();
    let state = avr.snapshot();
    assert_eq!(state.zone(Zone::Main).name.as_deref(), Some("MAIN ZONE"));
    assert_eq!(state.zone(Zone::Zone2).name.as_deref(), Some("Kitchen"));
    assert_eq!(state.device().friendly_name.as_deref(), Some("Denon AVR"));
}

#[tokio::test]
async fn update_with_appcommand_reads_tone_control() {
    for receiver_type in [ReceiverType::AvrX, ReceiverType::AvrX2016] {
        update_reads_tone_control(receiver_type).await;
    }
}

async fn update_reads_tone_control(receiver_type: ReceiverType) {
    let fake = avr_x_receiver().await;
    let avr = connect(&fake, receiver_type).await.unwrap();
    avr.update().await.unwrap();
    let request = fake
        .requests()
        .into_iter()
        .rev()
        .find(|seen| seen.path == "/goform/AppCommand.xml")
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&request.body)
            .matches("<cmd id=\"1\">")
            .count(),
        13
    );
    let state = avr.snapshot();
    assert_eq!(state.device().tone_control, Some(true));
    assert_eq!(state.zone(Zone::Main).bass, ToneLevel::from_db(3));
    assert_eq!(state.zone(Zone::Main).treble, Some(ToneLevel::MAIN_MIN));
    // The other values are still read.
    assert_eq!(state.zone(Zone::Main).power, Some(Power::On));
}

const DISTANCES_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-speakers-distances.xml");
const CHANNEL_LEVELS_PAGE: &[u8] = include_bytes!(
    "../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-option-channel-levels.xml"
);
const BASS_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-speakers-bass.xml");
const CROSSOVERS_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-speakers-crossovers.xml");

#[tokio::test]
async fn speaker_details_are_read_from_the_web_control_pages() {
    let receiver = avr_x_receiver().await;
    let web_interface = Fake::start(|seen| match seen.path.as_str() {
        "/ajax/speakers/get_config?type=4" => Reply::Body(DISTANCES_PAGE.to_vec()),
        "/ajax/speakers/get_config?type=6" => Reply::Body(CROSSOVERS_PAGE.to_vec()),
        "/ajax/option/get_config?type=3" => Reply::Body(CHANNEL_LEVELS_PAGE.to_vec()),
        "/ajax/speakers/get_config?type=7" => Reply::Body(BASS_PAGE.to_vec()),
        _ => Reply::Status(404),
    })
    .await;
    let avr = builder(&receiver, ReceiverType::AvrX)
        .web_interface_port(web_interface.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    assert!(avr.snapshot().device().speaker_setup.distances.is_none());
    let result = avr.update_speaker_details().await.unwrap();
    assert!(result.any_read());
    assert_eq!(result.distances, PageStatus::Read);
    assert_eq!(result.channel_levels, PageStatus::Read);
    let state = avr.snapshot();
    let setup = &state.device().speaker_setup;
    assert_eq!(setup.distances.as_ref().unwrap().entries.len(), 13);
    assert_eq!(setup.subwoofer_count, Some(2));
    assert_eq!(setup.crossovers.len(), 6);
    assert_eq!(setup.crossover_choices.len(), 13);
    assert_eq!(setup.bass.as_ref().unwrap().lpf_for_lfe, Some(120));
    // The 13 channels of the speaker setup are shown on the dial page.
    assert_eq!(state.device().channel_volumes.len(), 13);
    assert_eq!(state.device().active_channels.len(), 13);
    // The same pages again change nothing.
    let mut changes = avr.changes();
    assert!(avr.update_speaker_details().await.unwrap().any_read());
    assert!(changes.try_recv().is_err());
}

#[tokio::test]
async fn a_missing_speaker_page_is_not_an_error() {
    let receiver = avr_x_receiver().await;
    let web_interface = Fake::start(|seen| match seen.path.as_str() {
        "/ajax/speakers/get_config?type=6" => Reply::Body(CROSSOVERS_PAGE.to_vec()),
        _ => Reply::Status(404),
    })
    .await;
    let avr = builder(&receiver, ReceiverType::AvrX)
        .web_interface_port(web_interface.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    let result = avr.update_speaker_details().await.unwrap();
    assert!(result.any_read());
    assert_eq!(result.distances, PageStatus::NotFound);
    assert_eq!(result.crossovers, PageStatus::Read);
    let state = avr.snapshot();
    let setup = &state.device().speaker_setup;
    assert!(setup.distances.is_none());
    assert_eq!(setup.crossovers.len(), 6);
}

#[tokio::test]
async fn a_receiver_without_the_port_is_an_error_that_the_client_can_see() {
    let receiver = avr_x_receiver().await;
    // A port that nothing listens on.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let avr = builder(&receiver, ReceiverType::AvrX)
        .web_interface_port(port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    for _ in 0..2 {
        let error = avr.update_speaker_details().await.unwrap_err();
        assert!(error.is_unreachable(), "{error}");
    }
    assert!(avr.snapshot().device().speaker_setup.distances.is_none());
}

const INPUT_ASSIGN_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-inputs-assign.xml");
const SOURCE_NAMES_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-inputs-names.xml");

#[tokio::test]
async fn source_details_are_read_from_the_web_control_pages() {
    let receiver = avr_x_receiver().await;
    let web_interface = Fake::start(|seen| match seen.path.as_str() {
        "/ajax/inputs/get_config?type=2" => Reply::Body(INPUT_ASSIGN_PAGE.to_vec()),
        "/ajax/inputs/get_config?type=3" => Reply::Body(SOURCE_NAMES_PAGE.to_vec()),
        _ => Reply::Status(404),
    })
    .await;
    let avr = builder(&receiver, ReceiverType::AvrX)
        .web_interface_port(web_interface.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    let result = avr.update_source_details().await.unwrap();
    assert!(result.assignments.is_read() && result.names.is_read());
    let state = avr.snapshot();
    let apple = state.device().sources.get("MPLAY").unwrap();
    assert_eq!(apple.hdmi_input.as_deref(), Some("2"));
    let bluray = state.device().sources.get("BD").unwrap();
    assert_eq!(bluray.default_name.as_deref(), Some("Blu-ray"));
    assert_eq!(bluray.auto_name.as_deref(), Some("UDP-203"));
    assert_eq!(state.device().sources.max_name_length(), Some(16));
    // Reading the same pages again changes nothing.
    let mut changes = avr.changes();
    assert!(avr.update_source_details().await.unwrap().any_read());
    assert!(changes.try_recv().is_err());
}

#[tokio::test]
async fn missing_source_pages_are_skipped_and_a_closed_port_is_an_error() {
    let receiver = avr_x_receiver().await;
    let web_interface = Fake::start(|_| Reply::Status(404)).await;
    let avr = builder(&receiver, ReceiverType::AvrX)
        .web_interface_port(web_interface.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    let result = avr.update_source_details().await.unwrap();
    assert!(!result.any_read());
    assert_eq!(result.assignments, PageStatus::NotFound);
    assert!(avr.snapshot().device().sources.get("MPLAY").is_none());

    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let avr = builder(&receiver, ReceiverType::AvrX)
        .web_interface_port(port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    assert!(
        avr.update_source_details()
            .await
            .unwrap_err()
            .is_unreachable()
    );
}

#[tokio::test]
async fn refused_and_unreadable_pages_are_reported_per_page() {
    let receiver = avr_x_receiver().await;
    let web_interface = Fake::start(|seen| match seen.path.as_str() {
        "/ajax/speakers/get_config?type=4" => Reply::Status(500),
        "/ajax/speakers/get_config?type=6" => Reply::Body(b"<html>sorry</html>".to_vec()),
        "/ajax/option/get_config?type=3" => Reply::Status(403),
        "/ajax/speakers/get_config?type=7" => Reply::Body(BASS_PAGE.to_vec()),
        _ => Reply::Status(404),
    })
    .await;
    let avr = builder(&receiver, ReceiverType::AvrX)
        .web_interface_port(web_interface.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    let result = avr.update_speaker_details().await.unwrap();
    assert_eq!(result.distances, PageStatus::Refused(500));
    assert_eq!(result.crossovers, PageStatus::Unreadable);
    assert_eq!(result.channel_levels, PageStatus::Refused(403));
    assert_eq!(result.bass, PageStatus::Read);
    assert!(result.any_read());
}

#[tokio::test]
async fn settings_availability_are_read_from_the_web_control_pages() {
    const GROUPS: [(&str, &[u8]); 7] = [
        (
            "/ajax/audio/get_config?type=1",
            include_bytes!(
                "../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-availability-audio.xml"
            ),
        ),
        (
            "/ajax/video/get_config?type=1",
            include_bytes!(
                "../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-availability-video.xml"
            ),
        ),
        (
            "/ajax/inputs/get_config?type=1",
            include_bytes!(
                "../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-availability-inputs.xml"
            ),
        ),
        (
            "/ajax/speakers/get_config?type=1",
            include_bytes!(
                "../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-availability-speakers.xml"
            ),
        ),
        (
            "/ajax/network/get_config?type=1",
            include_bytes!(
                "../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-availability-network.xml"
            ),
        ),
        (
            "/ajax/general/get_config?type=1",
            include_bytes!(
                "../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-availability-general.xml"
            ),
        ),
        (
            "/ajax/option/get_config?type=1",
            include_bytes!(
                "../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-availability-option.xml"
            ),
        ),
    ];
    let receiver = avr_x_receiver().await;
    // The video group is missing on this fake receiver.
    let web_interface = Fake::start(|seen| {
        GROUPS
            .iter()
            .filter(|(path, _)| !path.contains("/video/"))
            .find(|(path, _)| *path == seen.path)
            .map_or(Reply::Status(404), |(_, body)| Reply::Body(body.to_vec()))
    })
    .await;
    let avr = builder(&receiver, ReceiverType::AvrX)
        .web_interface_port(web_interface.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    let result = avr.update_settings_availability().await.unwrap();
    assert!(result.any_read());
    assert_eq!(
        result.status(SettingsGroup::Speakers),
        Some(PageStatus::Read)
    );
    assert_eq!(
        result.status(SettingsGroup::Video),
        Some(PageStatus::NotFound)
    );
    let state = avr.snapshot();
    let crossovers = state
        .device()
        .settings_availability
        .item(SettingsGroup::Speakers, "Crossovers")
        .unwrap();
    assert_eq!(crossovers.availability, Availability::Blocked);
    assert_eq!(crossovers.reason_code, Some(3));
    assert!(
        state
            .device()
            .settings_availability
            .group(SettingsGroup::Video)
            .is_none()
    );
    // The same pages again change nothing.
    let mut changes = avr.changes();
    assert!(avr.update_settings_availability().await.unwrap().any_read());
    assert!(changes.try_recv().is_err());
}

#[tokio::test]
async fn setting_details_are_read_from_the_detail_pages() {
    let receiver = avr_x_receiver().await;
    // The HDMI setup page is missing on this fake receiver.
    let web_interface = Fake::start(|seen| match seen.path.as_str() {
        "/ajax/audio/get_config?type=4" => Reply::Body(
            include_bytes!(
                "../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-details-surround.xml"
            )
            .to_vec(),
        ),
        "/ajax/video/get_config?type=4" => Reply::Body(
            include_bytes!(
                "../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-details-output.xml"
            )
            .to_vec(),
        ),
        _ => Reply::Status(404),
    })
    .await;
    let avr = builder(&receiver, ReceiverType::AvrX)
        .web_interface_port(web_interface.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    let result = avr.update_detailed_availability().await.unwrap();
    assert_eq!(
        result.status(SettingsGroup::SurroundParameters),
        Some(PageStatus::Read)
    );
    assert_eq!(
        result.status(SettingsGroup::HdmiSetup),
        Some(PageStatus::NotFound)
    );
    let state = avr.snapshot();
    let settings = &state.device().settings_availability;
    assert_eq!(
        settings
            .item(SettingsGroup::SurroundParameters, "DialogControl")
            .unwrap()
            .availability,
        Availability::NotOffered
    );
    assert_eq!(
        settings
            .item(SettingsGroup::OutputSettings, "HDMIUpscaler")
            .unwrap()
            .availability,
        Availability::Blocked
    );
    // Only the pages of the details were asked for.
    assert!(
        web_interface
            .requests()
            .iter()
            .all(|seen| seen.path.contains("type=4") || seen.path.contains("type=3"))
    );
}

#[tokio::test]
async fn system_settings_are_read_from_the_web_control_page() {
    let receiver = avr_x_receiver().await;
    let web_interface = Fake::start(|seen| match seen.path.as_str() {
        "/ajax/advanced/get_config?type=3" => Reply::Body(
            include_bytes!(
                "../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-system-settings.xml"
            )
            .to_vec(),
        ),
        _ => Reply::Status(404),
    })
    .await;
    let avr = builder(&receiver, ReceiverType::AvrX)
        .web_interface_port(web_interface.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    assert_eq!(
        avr.update_system_settings().await.unwrap(),
        PageStatus::Read
    );
    let settings = avr.snapshot().device().system_settings;
    assert_eq!(settings.network_control, Some(NetworkControl::Always));
    assert_eq!(settings.allow_update, Some(true));
    assert_eq!(settings.hdmi_control, Some(false));
    // The same page again changes nothing.
    let mut changes = avr.changes();
    avr.update_system_settings().await.unwrap();
    assert!(changes.try_recv().is_err());
}

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!(
            "../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-",
            $name,
            ".xml"
        ))
    };
}

async fn avr_with_web_interface(pages: &'static [(&'static str, &'static [u8])]) -> (Avr, Fake) {
    let receiver = avr_x_receiver().await;
    let web = web_interface_with_pages(pages).await;
    let avr = builder(&receiver, ReceiverType::AvrX)
        .web_interface_port(web.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    (avr, web)
}

#[tokio::test]
async fn the_video_menu_is_read_from_its_pages() {
    use denon_marantz_proto::{HdcpMode, OsdVolume, SignalFormat, TvFormat, VideoMenuPage};
    // The TV format page is missing on this fake receiver.
    static PAGES: [(&str, &[u8]); 5] = [
        ("/ajax/video/get_config?type=7", fixture!("video-osd")),
        ("/ajax/video/get_config?type=11", fixture!("video-hdcp")),
        (
            "/ajax/video/get_config?type=12",
            fixture!("video-screen-saver"),
        ),
        (
            "/ajax/video/get_config?type=13",
            fixture!("video-hdmi-scaler"),
        ),
        (
            "/ajax/video/get_config?type=14",
            fixture!("video-4k8k-per-input"),
        ),
    ];
    let (avr, _web) = avr_with_web_interface(&PAGES).await;
    let result = avr.update_video_details().await.unwrap();
    assert!(result.any_read());
    assert_eq!(
        result.status(VideoMenuPage::TvFormat),
        Some(PageStatus::NotFound)
    );
    assert_eq!(result.status(VideoMenuPage::Hdcp), Some(PageStatus::Read));
    let state = avr.snapshot();
    let menu = &state.device().video_menu;
    assert_eq!(menu.on_screen_display.unwrap().volume, Some(OsdVolume::Off));
    assert_eq!(menu.tv_format, None::<TvFormat>);
    assert_eq!(menu.screen_saver, Some(false));
    assert_eq!(menu.hdmi_upscaler, Some(false));
    assert_eq!(menu.hdcp[0].value, Some(HdcpMode::Auto));
    assert_eq!(menu.signal_format[0].value, Some(SignalFormat::Enhanced8k));
    // The same pages again change nothing.
    let mut changes = avr.changes();
    avr.update_video_details().await.unwrap();
    assert!(changes.try_recv().is_err());
}

#[tokio::test]
async fn the_general_settings_are_read_from_their_pages() {
    use denon_marantz_proto::{GeneralPage, Language, Trigger, TriggerItemKind};
    static PAGES: [(&str, &[u8]); 6] = [
        (
            "/ajax/general/get_config?type=2",
            fixture!("general-language"),
        ),
        (
            "/ajax/general/get_config?type=11",
            fixture!("general-firmware"),
        ),
        (
            "/ajax/general/get_config?type=14",
            fixture!("general-usage-data"),
        ),
        (
            "/ajax/general/get_config?type=8",
            fixture!("general-trigger-a"),
        ),
        (
            "/ajax/general/get_config?type=9",
            fixture!("general-trigger-b"),
        ),
        (
            "/ajax/general/get_config?type=20",
            fixture!("general-trigger-c"),
        ),
    ];
    let (avr, _web) = avr_with_web_interface(&PAGES).await;
    let result = avr.update_general_settings().await.unwrap();
    assert_eq!(result.pages.len(), GeneralPage::ALL.len());
    assert!(result.pages.iter().all(|(_, status)| status.is_read()));
    let state = avr.snapshot();
    let general = &state.device().general_settings;
    assert_eq!(general.language, Some(Language::English));
    assert_eq!(general.auto_update, Some(false));
    assert_eq!(general.time_zone, Some(28));
    assert_eq!(general.usage_data, Some(false));
    assert_eq!(general.triggers.len(), 3);
    let first = general.trigger(Trigger::One).unwrap();
    let source = first
        .enabled()
        .find(|item| item.kind == TriggerItemKind::Source);
    assert_eq!(source.unwrap().source, Some("BD"));
}

#[tokio::test]
async fn speaker_details_and_source_details_have_the_new_pages() {
    use denon_marantz_proto::XlrTarget;
    static PAGES: [(&str, &[u8]); 3] = [
        (
            "/ajax/speakers/get_config?type=9",
            fixture!("speakers-playback-2ch"),
        ),
        (
            "/ajax/speakers/get_config?type=12",
            fixture!("speakers-xlr-assign"),
        ),
        (
            "/ajax/inputs/get_config?type=5",
            fixture!("inputs-source-levels"),
        ),
    ];
    let (avr, _web) = avr_with_web_interface(&PAGES).await;
    let speakers = avr.update_speaker_details().await.unwrap();
    assert_eq!(speakers.playback_2ch, PageStatus::Read);
    assert_eq!(speakers.xlr_assign, PageStatus::Read);
    assert!(speakers.any_read());
    let sources = avr.update_source_details().await.unwrap();
    assert_eq!(sources.levels, PageStatus::Read);
    assert_eq!(sources.assignments, PageStatus::NotFound);
    let state = avr.snapshot();
    let device = state.device();
    assert_eq!(device.playback_2ch.as_ref().unwrap().distances.len(), 2);
    assert_eq!(
        device.xlr_assign.as_ref().unwrap().outputs[0].choices[0],
        XlrTarget::Subwoofer
    );
    assert_eq!(device.source_levels.analog_db, Some(0));
}

fn audyssey_requests(fake: &Fake) -> usize {
    fake.requests()
        .iter()
        .filter(|seen| seen.path == "/goform/AppCommand0300.xml")
        .count()
}

#[tokio::test]
async fn update_of_a_2016_receiver_reads_the_audyssey_settings() {
    let answer = appcommand0300_answer(
        "<param name=\"dynamiceq\" control=\"1\">1</param>\
        <param name=\"reflevoffset\" control=\"1\">5</param>\
        <param name=\"dynamicvol\" control=\"1\">2</param>\
        <param name=\"multeq\" control=\"1\">2</param>",
    );
    let answer: &'static str = Box::leak(answer.into_boxed_str());
    let fake = receiver_with_audyssey(Some(answer), true).await;
    let avr = connect(&fake, ReceiverType::AvrX2016).await.unwrap();
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Read
    );
    let state = avr.snapshot();
    let device = state.device();
    assert_eq!(device.dynamic_eq, Some(true));
    assert!(device.multi_eq.is_some());
    assert!(device.reference_level_offset.is_some());
    assert!(device.dynamic_volume.is_some());
}

#[tokio::test]
async fn update_of_a_2016_receiver_reads_the_signal() {
    let answer = appcommand0300_answer("");
    let answer: &'static str = Box::leak(answer.into_boxed_str());
    let fake = receiver_with_audyssey(Some(answer), true).await;
    let avr = connect(&fake, ReceiverType::AvrX2016).await.unwrap();
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Read
    );
    let state = avr.snapshot();
    let signal = &state.device().signal;
    assert_eq!(signal.video.input.as_deref(), Some("4K50"));
    // `---` is no signal.
    assert_eq!(signal.video.output, None);
    // The text of the receiver is trimmed, and the unit of the rate is left out.
    assert_eq!(signal.audio_signal.as_deref(), Some("Dolby Atmos"));
    assert_eq!(signal.audio_sound.as_deref(), Some("Stereo"));
    assert_eq!(signal.sampling_rate.as_deref(), Some("44.1"));
    assert_eq!(signal.channels.input.unwrap().to_string(), "2.0");
    assert_eq!(signal.channels.output.unwrap().to_string(), "2.1.1");
    // Audyssey values that are blank are unknown.
    assert_eq!(state.device().dynamic_eq, None);
}

#[tokio::test]
async fn blank_audyssey_values_are_unknown() {
    // A receiver that is calibrated with Dirac answers with blank values.
    let answer = appcommand0300_answer(
        "<param name=\"dynamiceq\" control=\"0\"></param>\
        <param name=\"reflevoffset\" control=\"0\"></param>\
        <param name=\"dynamicvol\" control=\"0\"></param>\
        <param name=\"multeq\" control=\"0\"></param>",
    );
    let answer: &'static str = Box::leak(answer.into_boxed_str());
    let fake = receiver_with_audyssey(Some(answer), true).await;
    let avr = connect(&fake, ReceiverType::AvrX2016).await.unwrap();
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Read
    );
    let state = avr.snapshot();
    assert_eq!(state.device().dynamic_eq, None);
    assert_eq!(state.device().multi_eq, None);
}

#[tokio::test]
async fn a_receiver_without_the_audyssey_query_is_asked_once() {
    let fake = receiver_with_audyssey(None, true).await;
    let avr = connect(&fake, ReceiverType::AvrX2016).await.unwrap();
    // The missing query is not an error, and it is not asked again.
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::NotFound
    );
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::NotFound
    );
    assert_eq!(audyssey_requests(&fake), 1);
}

#[tokio::test]
async fn an_error_item_keeps_the_channels_that_the_state_has() {
    let good = appcommand0300_answer("");
    let bad = good.replace(&appcommand0300_item("GetInputSignal"), "<error>2</error>");
    let fake = receiver_with_signal_answers(true, move |asked| {
        Reply::Body(if asked == 0 { &good } else { &bad }.clone().into_bytes())
    })
    .await;
    let avr = connect(&fake, ReceiverType::AvrX2016).await.unwrap();
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Read
    );
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Read
    );
    let state = avr.snapshot();
    let signal = &state.device().signal;
    assert_eq!(signal.channels.input.unwrap().to_string(), "2.0");
    assert_eq!(signal.channels.output.unwrap().to_string(), "2.1.1");
}

/// The bodies of the requests to `AppCommand0300.xml`.
fn signal_requests(fake: &Fake) -> Vec<String> {
    fake.requests()
        .into_iter()
        .filter(|seen| seen.path == "/goform/AppCommand0300.xml")
        .map(|seen| String::from_utf8_lossy(&seen.body).into_owned())
        .collect()
}

#[tokio::test]
async fn a_receiver_without_the_output_settings_is_not_asked_for_them() {
    let answer: &'static str = Box::leak(appcommand0300_answer("").into_boxed_str());
    let fake = receiver_with_audyssey(Some(answer), true).await;
    let avr = connect(&fake, ReceiverType::AvrX2016).await.unwrap();
    assert!(!avr.info().hdmi_output_over_http);
    assert_eq!(avr.info().queries, None);
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Read
    );
    let bodies = signal_requests(&fake);
    assert!(
        bodies
            .iter()
            .all(|body| !body.contains("GetOutputSettings")),
        "{bodies:?}"
    );
}

#[tokio::test]
async fn update_of_a_receiver_with_the_output_settings_reads_the_hdmi_output() {
    let answer = appcommand0300_answer("").replace("</rx>", &(output_settings_item("2") + "</rx>"));
    let fake = receiver_with_info_and_signal_answers(AVR_X_OUTPUT_SETTINGS_INFO, true, move |_| {
        Reply::Body(answer.clone().into_bytes())
    })
    .await;
    let avr = connect(&fake, ReceiverType::AvrX2016).await.unwrap();
    assert!(avr.info().hdmi_output_over_http);
    assert!(
        avr.info()
            .queries
            .is_some_and(|queries| queries.output_settings)
    );
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Read
    );
    let state = avr.snapshot();
    assert_eq!(
        state.device().video_settings.hdmi_output,
        Some(HdmiOutput::Hdmi2)
    );
    assert_eq!(state.device().signal.video.input.as_deref(), Some("4K50"));
}

#[tokio::test]
async fn an_error_item_for_the_output_settings_keeps_the_signal() {
    let answer = appcommand0300_answer("").replace("</rx>", "<error>2</error></rx>");
    let fake = receiver_with_info_and_signal_answers(AVR_X_OUTPUT_SETTINGS_INFO, true, move |_| {
        Reply::Body(answer.clone().into_bytes())
    })
    .await;
    let avr = connect(&fake, ReceiverType::AvrX2016).await.unwrap();
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Read
    );
    let state = avr.snapshot();
    assert_eq!(state.device().video_settings.hdmi_output, None);
    assert_eq!(state.device().signal.video.input.as_deref(), Some("4K50"));
}

#[tokio::test]
async fn a_short_answer_gives_the_items_that_it_has() {
    let good = appcommand0300_answer("");
    // A busy receiver answers only the video item, with another resolution.
    let short =
        format!("<rx>{}</rx>", appcommand0300_item("GetVideoInfo")).replace("4K50", "1080p60");
    let fake = receiver_with_signal_answers(true, move |asked| {
        Reply::Body(if asked == 0 { &good } else { &short }.clone().into_bytes())
    })
    .await;
    let avr = connect(&fake, ReceiverType::AvrX2016).await.unwrap();
    for _ in 0..2 {
        assert_eq!(
            avr.update_signal_and_settings().await.unwrap(),
            PageStatus::Read
        );
    }
    let state = avr.snapshot();
    let signal = &state.device().signal;
    assert_eq!(signal.video.input.as_deref(), Some("1080p60"));
    assert_eq!(signal.audio_sound.as_deref(), Some("Stereo"));
    assert_eq!(signal.channels.input.unwrap().to_string(), "2.0");
}

#[tokio::test]
async fn a_refusal_that_is_not_a_404_does_not_stop_the_asking() {
    let good = appcommand0300_answer("");
    let fake = receiver_with_signal_answers(true, move |asked| {
        if asked == 0 {
            Reply::Status(503)
        } else {
            Reply::Body(good.clone().into_bytes())
        }
    })
    .await;
    let avr = connect(&fake, ReceiverType::AvrX2016).await.unwrap();
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Refused(503)
    );
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Read
    );
    assert_eq!(audyssey_requests(&fake), 2);
}

#[tokio::test]
async fn a_receiver_with_status_pages_is_not_asked_for_audyssey() {
    let fake = old_receiver().await;
    let avr = connect(&fake, ReceiverType::Avr).await.unwrap();
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::NotFound
    );
    assert_eq!(audyssey_requests(&fake), 0);
}

#[tokio::test]
async fn an_older_receiver_with_appcommand_is_asked_for_audyssey() {
    let answer: &'static str = Box::leak(
        appcommand0300_answer("<param name=\"dynamiceq\" control=\"1\">1</param>").into_boxed_str(),
    );
    let fake = receiver_with_audyssey(Some(answer), true).await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Read
    );
    assert_eq!(avr.snapshot().device().dynamic_eq, Some(true));
}

/// The `AppCommand` requests for the update, and the number of commands in each.
fn update_sizes(fake: &Fake) -> Vec<usize> {
    fake.requests()
        .iter()
        .filter(|seen| seen.path == "/goform/AppCommand.xml")
        .map(|seen| {
            String::from_utf8_lossy(&seen.body)
                .matches("<cmd id=")
                .count()
        })
        .filter(|&count| count > 1)
        .collect()
}

#[tokio::test]
async fn an_update_without_the_tone_item_drops_tone_control() {
    // The receiver answers the update without an item for tone control.
    let fake = receiver_with_audyssey(None, false).await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    avr.update().await.unwrap();
    assert_eq!(update_sizes(&fake), [13, 12]);
    // It is not asked again.
    avr.update().await.unwrap();
    assert_eq!(update_sizes(&fake), [13, 12, 12]);
    assert_eq!(avr.snapshot().zone(Zone::Main).power, Some(Power::On));
}

/// A receiver that refuses a request with tone control.
async fn receiver_that_refuses_tone_control() -> Fake {
    receiver_with_info_that_refuses_tone_control(AVR_X_INFO).await
}

/// Like [`receiver_that_refuses_tone_control`], with this device information.
async fn receiver_with_info_that_refuses_tone_control(info: &'static [u8]) -> Fake {
    Fake::start(
        move |seen| match (seen.method.as_str(), seen.path.as_str()) {
            ("GET", "/goform/Deviceinfo.xml") => Reply::Body(info.to_vec()),
            ("POST", _) if is_friendly_name_request(&seen.body) => {
                Reply::Body(friendly_name_answer("X"))
            }
            ("POST", _) if String::from_utf8_lossy(&seen.body).contains("GetToneControl") => {
                Reply::Status(500)
            }
            ("POST", _) => Reply::Body(UPDATE_ANSWER.to_vec()),
            _ => Reply::Status(404),
        },
    )
    .await
}

#[tokio::test]
async fn an_update_that_is_refused_with_tone_control_drops_it() {
    let fake = receiver_that_refuses_tone_control().await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    avr.update().await.unwrap();
    avr.update().await.unwrap();
    assert_eq!(update_sizes(&fake), [13, 12, 12]);
}

#[tokio::test]
async fn a_2016_receiver_keeps_asking_for_tone_control() {
    // All 2016 models have tone control, so a refusal is not a reason to drop it.
    let fake = receiver_that_refuses_tone_control().await;
    let avr = connect(&fake, ReceiverType::AvrX2016).await.unwrap();
    assert!(avr.update().await.unwrap_err().is_status());
    assert!(avr.update().await.unwrap_err().is_status());
    assert_eq!(update_sizes(&fake), [13, 13]);
}

#[tokio::test]
async fn a_receiver_that_lists_tone_control_keeps_asking_for_it() {
    let fake = receiver_with_info_that_refuses_tone_control(AVR_X_OUTPUT_SETTINGS_INFO).await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    assert!(avr.update().await.unwrap_err().is_status());
    assert!(avr.update().await.unwrap_err().is_status());
    assert_eq!(update_sizes(&fake), [13, 13]);
}

#[tokio::test]
async fn a_receiver_that_lists_no_tone_control_is_not_asked_for_it() {
    let fake = receiver_with_info_that_refuses_tone_control(AUDIO_INFO_ONLY_INFO).await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    avr.update().await.unwrap();
    assert_eq!(update_sizes(&fake), [12]);
}

#[tokio::test]
async fn only_the_listed_signal_queries_are_sent() {
    let answer = format!("<rx>{}</rx>", appcommand0300_item("GetAudioInfo"));
    let fake = receiver_with_info_and_signal_answers(AUDIO_INFO_ONLY_INFO, false, move |_| {
        Reply::Body(answer.clone().into_bytes())
    })
    .await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    let queries: ListedQueries = avr.info().queries.unwrap();
    assert!(queries.audio_info && !queries.video_info && !queries.tone_control);
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Read
    );
    let bodies = signal_requests(&fake);
    assert_eq!(bodies.len(), 1);
    assert!(bodies[0].contains("GetAudioInfo"), "{}", bodies[0]);
    for query in [
        "GetAudyssey",
        "GetVideoInfo",
        "GetInputSignal",
        "GetActiveSpeaker",
    ] {
        assert!(!bodies[0].contains(query), "{query}");
    }
    assert_eq!(
        avr.snapshot().device().signal.audio_sound.as_deref(),
        Some("Stereo")
    );
}

#[tokio::test]
async fn a_receiver_that_lists_no_signal_query_is_not_asked() {
    const INFO: &[u8] = b"<Device_Info><CommApiVers>0300</CommApiVers>\
        <DeviceCapabilities><Setup><StatusInfo><Commands/></StatusInfo></Setup>\
        </DeviceCapabilities></Device_Info>";
    let fake = receiver_with_info_and_signal_answers(INFO, false, |_| Reply::Status(500)).await;
    let avr = connect(&fake, ReceiverType::AvrX).await.unwrap();
    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::NotFound
    );
    assert_eq!(signal_requests(&fake).len(), 0);
}
