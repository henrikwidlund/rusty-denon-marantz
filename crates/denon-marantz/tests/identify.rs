//! Receiver identification against fake receivers.

#![cfg(feature = "http")]

mod common;

use common::{CLOSED_PORT, Fake, OTHER_CLOSED_PORT, Reply, config};
use denon_marantz::{TypeProof, identify_on};
use denon_marantz_proto::ReceiverType;

const AVR_X_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0301</CommApiVers><ModelName>AVC-X3700H</ModelName>\
    <DeviceZones>2</DeviceZones><Caps><DiracLive/></Caps></Device_Info>";
/// Made up: no capture has readable XML without the AVR-X marks.
const OLD_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0100</CommApiVers><ModelName>AVR-FAKE</ModelName>\
    <DeviceZones>1</DeviceZones></Device_Info>";
const OLD_MODEL_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/AVR-1912-Deviceinfo.xml");

#[tokio::test]
async fn first_avr_x_answer_wins() {
    let silent = Fake::start(|_| Reply::Hang).await;
    let old = Fake::start(|_| Reply::Body(AVR_X_INFO.to_vec())).await;
    let candidates = [
        (ReceiverType::AvrX2016, silent.port),
        (ReceiverType::AvrX, old.port),
    ];
    let started = std::time::Instant::now();
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::AvrX);
    assert_eq!(found.port, old.port);
    assert_eq!(found.device_info.model_name.as_deref(), Some("AVC-X3700H"));
    assert_eq!(found.device_info.device_zones, Some(2));
    assert!(found.device_info.dirac_live);
    assert_eq!(found.type_proof, TypeProof::Read);
    // The silent port is not waited for.
    assert!(started.elapsed() < config().request_timeout);
}

#[tokio::test]
async fn first_type_on_a_port_is_used() {
    let fake = Fake::start(|_| Reply::Body(AVR_X_INFO.to_vec())).await;
    let candidates = [
        (ReceiverType::AvrX2016, fake.port),
        (ReceiverType::AvrX, fake.port),
    ];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::AvrX2016);
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn one_probe_sends_one_request() {
    let fake = Fake::start(|_| Reply::Body(AVR_X_INFO.to_vec())).await;
    let candidates = [(ReceiverType::AvrX2016, fake.port)];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::AvrX2016);
    assert_eq!(found.port, fake.port);
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn old_receiver_on_a_shared_port_keeps_its_device_info() {
    let fake = Fake::start(|_| Reply::Body(OLD_INFO.to_vec())).await;
    let candidates = [
        (ReceiverType::AvrX, fake.port),
        (ReceiverType::Avr, fake.port),
    ];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::Avr);
    assert_eq!(found.port, fake.port);
    assert_eq!(found.device_info.model_name.as_deref(), Some("AVR-FAKE"));
    assert_eq!(found.type_proof, TypeProof::Read);
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn falls_back_to_the_next_port() {
    let open = Fake::start(|_| Reply::Body(AVR_X_INFO.to_vec())).await;
    let candidates = [
        (ReceiverType::AvrX2016, CLOSED_PORT),
        (ReceiverType::AvrX, open.port),
    ];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::AvrX);
    assert_eq!(found.port, open.port);
}

#[tokio::test]
async fn old_model_page_means_avr() {
    let fake = Fake::start(|_| Reply::Body(OLD_MODEL_PAGE.to_vec())).await;
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
    assert_eq!(found.type_proof, TypeProof::OldModelPage);
    // Each port is asked once.
    assert_eq!(fake.requests().len(), 1);
}

#[tokio::test]
async fn http_errors_mean_avr() {
    let fake = Fake::start(|_| Reply::Status(404)).await;
    let candidates = [(ReceiverType::AvrX, fake.port)];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::Avr);
    assert_eq!(found.type_proof, TypeProof::Failed);
}

#[tokio::test]
async fn server_errors_prove_nothing() {
    let new = Fake::start(|_| Reply::Status(500)).await;
    let old = Fake::start(|_| Reply::Status(500)).await;
    let candidates = [
        (ReceiverType::AvrX2016, new.port),
        (ReceiverType::AvrX, old.port),
    ];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::Avr);
    assert_eq!(found.type_proof, TypeProof::Failed);
}

#[tokio::test]
async fn other_html_proves_nothing() {
    let fake = Fake::start(|_| Reply::Body(b"<html><body>Login</body></html>".to_vec())).await;
    let candidates = [(ReceiverType::AvrX, fake.port)];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.type_proof, TypeProof::Failed);
}

#[tokio::test]
async fn avr_x_on_one_port_is_read_after_a_redirect_on_another() {
    let new = Fake::start(|_| Reply::Body(AVR_X_INFO.to_vec())).await;
    let old = Fake::start(|_| Reply::Status(302)).await;
    let candidates = [
        (ReceiverType::AvrX2016, new.port),
        (ReceiverType::AvrX, old.port),
    ];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::AvrX2016);
    assert_eq!(found.type_proof, TypeProof::Read);
}

#[tokio::test]
async fn old_model_page_on_one_port_wins_over_an_error_on_another() {
    let new = Fake::start(|_| Reply::Status(403)).await;
    let old = Fake::start(|_| Reply::Body(OLD_MODEL_PAGE.to_vec())).await;
    let candidates = [
        (ReceiverType::AvrX2016, new.port),
        (ReceiverType::AvrX, old.port),
    ];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::Avr);
    assert_eq!(found.type_proof, TypeProof::OldModelPage);
}

#[tokio::test]
async fn old_model_page_wins_over_no_answer_on_another_port() {
    let old = Fake::start(|_| Reply::Body(OLD_MODEL_PAGE.to_vec())).await;
    let candidates = [
        (ReceiverType::AvrX2016, CLOSED_PORT),
        (ReceiverType::AvrX, old.port),
    ];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::Avr);
    assert_eq!(found.type_proof, TypeProof::OldModelPage);
}

#[tokio::test]
async fn no_answer_on_one_port_and_an_error_on_another_proves_nothing() {
    let old = Fake::start(|_| Reply::Status(404)).await;
    let candidates = [
        (ReceiverType::AvrX2016, CLOSED_PORT),
        (ReceiverType::AvrX, old.port),
    ];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::Avr);
    assert_eq!(found.type_proof, TypeProof::Failed);
}

#[tokio::test]
async fn device_info_of_an_old_receiver_is_kept() {
    let fake = Fake::start(|_| Reply::Body(OLD_INFO.to_vec())).await;
    let candidates = [(ReceiverType::AvrX, fake.port)];
    let found = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap();
    assert_eq!(found.receiver_type, ReceiverType::Avr);
    assert_eq!(found.device_info.model_name.as_deref(), Some("AVR-FAKE"));
    assert_eq!(found.device_info.device_zones, Some(1));
    assert_eq!(found.type_proof, TypeProof::Read);
}

#[tokio::test]
async fn nothing_answering_is_an_error() {
    let candidates = [
        (ReceiverType::AvrX2016, CLOSED_PORT),
        (ReceiverType::AvrX, OTHER_CLOSED_PORT),
    ];
    let error = identify_on("127.0.0.1", &candidates, config())
        .await
        .unwrap_err();
    // Windows reports a refused connection after about two seconds, so the
    // short timeout of the test can come first.
    assert!(
        error.is_network() || (cfg!(windows) && error.is_timeout()),
        "{error:?}"
    );
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

#[test]
fn proven_types() {
    assert!(TypeProof::Read.is_proven());
    assert!(TypeProof::OldModelPage.is_proven());
    assert!(!TypeProof::Failed.is_proven());
}
