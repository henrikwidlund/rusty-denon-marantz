//! `Avr` against fake receivers.

#![cfg(feature = "http")]

mod common;

use common::{
    AVR_X_INFO, Fake, Reply, UPDATE_ANSWER, avr_x_receiver, builder, config, friendly_name_answer,
    is_friendly_name_request,
};
use denon_marantz::{Avr, Error, UpdateMethod};
use denon_marantz_proto::{
    AutoStandby, Brand, DimmerMode, EcoMode, Power, ReceiverType, ToneLevel, Volume, Zone,
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

const OLD_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0100</CommApiVers><ModelName>AVR-3313</ModelName>\
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
        12
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
async fn silence_during_setup_is_an_error() {
    let fake = Fake::start(|seen| match seen.path.as_str() {
        "/goform/Deviceinfo.xml" => Reply::Body(AVR_X_INFO.to_vec()),
        "/description.xml" => Reply::Hang,
        _ => Reply::Status(404),
    })
    .await;
    let error = connect(&fake, ReceiverType::AvrX).await.unwrap_err();
    assert!(error.is_timeout(), "{error:?}");
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
async fn update_of_a_2016_receiver_reads_tone_control() {
    let fake = avr_x_receiver().await;
    let avr = connect(&fake, ReceiverType::AvrX2016).await.unwrap();
    avr.update().await.unwrap();
    let last = fake.requests().pop().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&last.body)
            .matches("<cmd id=\"1\">")
            .count(),
        13
    );
    let state = avr.snapshot();
    assert_eq!(state.device().tone_control, Some(true));
    assert_eq!(state.zone(Zone::Main).bass, ToneLevel::from_db(3));
    assert_eq!(state.zone(Zone::Main).treble, Some(ToneLevel::MIN));
    // The other values are still read.
    assert_eq!(state.zone(Zone::Main).power, Some(Power::On));
}
