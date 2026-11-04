//! Receiver identification against fake receivers.

#![cfg(feature = "http")]

mod common;

use std::time::Duration;

use common::{Fake, Reply};
use denon_marantz::{HttpConfig, Pacing, identify_on};
use denon_marantz_proto::ReceiverType;

fn config() -> HttpConfig {
    HttpConfig {
        connect_timeout: Duration::from_millis(300),
        request_timeout: Duration::from_millis(300),
        pacing: Pacing::Fixed {
            interval: Duration::ZERO,
        },
        ..HttpConfig::default()
    }
}

const AVR_X_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0301</CommApiVers><ModelName>AVC-X3700H</ModelName>\
    <DeviceZones>2</DeviceZones><Caps><DiracLive/></Caps></Device_Info>";
const OLD_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0100</CommApiVers><ModelName>AVR-1713</ModelName>\
    <DeviceZones>1</DeviceZones></Device_Info>";
const HTML: &[u8] = b"<html><body>Form Deviceinfo.xml is not defined</body></html>";

fn unused_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[tokio::test]
async fn newest_generation_is_tried_first() {
    let new = Fake::start(|_| Reply::Body(AVR_X_INFO.to_vec())).await;
    let old = Fake::start(|_| Reply::Body(AVR_X_INFO.to_vec())).await;
    let candidates = [
        (ReceiverType::AvrX2016, new.port),
        (ReceiverType::AvrX, old.port),
    ];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::AvrX2016);
    assert_eq!(found.port, new.port);
    assert_eq!(found.device_info.model_name.as_deref(), Some("AVC-X3700H"));
    assert_eq!(found.device_info.device_zones, Some(2));
    assert!(found.device_info.dirac_live);
    assert_eq!(old.requests().len(), 0);
}

#[tokio::test]
async fn falls_back_to_the_next_port() {
    let closed = unused_port();
    let open = Fake::start(|_| Reply::Body(AVR_X_INFO.to_vec())).await;
    let candidates = [
        (ReceiverType::AvrX2016, closed),
        (ReceiverType::AvrX, open.port),
    ];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::AvrX);
    assert_eq!(found.port, open.port);
}

#[tokio::test]
async fn error_pages_mean_avr() {
    let fake = Fake::start(|_| Reply::Body(HTML.to_vec())).await;
    let candidates = [
        (ReceiverType::AvrX2016, fake.port),
        (ReceiverType::AvrX, fake.port),
    ];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::Avr);
    assert_eq!(found.port, 80);
    assert_eq!(found.device_info.model_name, None);
    assert_eq!(fake.requests().len(), 2);
}

#[tokio::test]
async fn http_errors_mean_avr() {
    let fake = Fake::start(|_| Reply::Status(404)).await;
    let candidates = [(ReceiverType::AvrX, fake.port)];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::Avr);
}

#[tokio::test]
async fn device_info_of_an_old_receiver_is_kept() {
    let fake = Fake::start(|_| Reply::Body(OLD_INFO.to_vec())).await;
    let candidates = [(ReceiverType::AvrX, fake.port)];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::Avr);
    assert_eq!(found.device_info.model_name.as_deref(), Some("AVR-1713"));
    assert_eq!(found.device_info.device_zones, Some(1));
}

#[tokio::test]
async fn nothing_answering_is_an_error() {
    let candidates = [
        (ReceiverType::AvrX2016, unused_port()),
        (ReceiverType::AvrX, unused_port()),
    ];
    let error = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap_err();
    assert!(error.is_network(), "{error:?}");
}

#[tokio::test]
async fn silence_is_a_timeout_error() {
    let fake = Fake::start(|_| Reply::Hang).await;
    let candidates = [(ReceiverType::AvrX, fake.port)];
    let error = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap_err();
    assert!(error.is_timeout(), "{error:?}");
}
