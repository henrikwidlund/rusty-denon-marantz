//! Tests of `discover` with a fake SSDP port and a fake receiver.

#![cfg(feature = "discovery")]

mod common;

use std::net::Ipv4Addr;
use std::time::Duration;

use common::{
    AV_RECEIVER_INFO, Fake, FakeSsdp, Reply, SOUND_BAR_INFO, avr_x_receiver,
    avr_x_receiver_with_info, ssdp_answer,
};
use denon_marantz::{DiscoveryConfig, Error, LocationPolicy, TypeProof, discover};
use denon_marantz_proto::{Brand, ReceiverType};

const LOOPBACK: Ipv4Addr = Ipv4Addr::LOCALHOST;

/// Settings that search the fake ports and wait a short time.
fn config(ssdp: &FakeSsdp, http: &Fake) -> DiscoveryConfig {
    DiscoveryConfig {
        wait: Duration::from_millis(300),
        interfaces: Some(vec![LOOPBACK]),
        target: ssdp.addr,
        probes: Some(vec![(ReceiverType::AvrX, http.port)]),
        ..DiscoveryConfig::default()
    }
}

fn location(http: &Fake) -> String {
    format!("http://127.0.0.1:{}/description.xml", http.port)
}

#[tokio::test]
async fn a_receiver_that_answers_is_found() {
    let http = avr_x_receiver().await;
    let ssdp = FakeSsdp::start(vec![ssdp_answer(&location(&http))]).await;
    let found = discover(&config(&ssdp, &http)).await.unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    let receiver = &found[0];
    assert_eq!(receiver.host, "127.0.0.1");
    assert_eq!(receiver.brand, Brand::Denon);
    assert_eq!(receiver.manufacturer, "Denon");
    assert_eq!(receiver.model_name.as_deref(), Some("Denon AVR-X1600H"));
    assert_eq!(receiver.serial_number.as_deref(), Some("XXX"));
    assert_eq!(receiver.mac_address.as_deref(), Some("0123456789AB"));
    assert_eq!(receiver.type_proof, TypeProof::Read);
    assert!(receiver.friendly_name.is_some());
    // One search for each target.
    let requests = ssdp.requests();
    assert_eq!(requests.len(), 3, "{requests:?}");
    for target in [
        "ssdp:all",
        "upnp:rootdevice",
        "urn:schemas-upnp-org:device:MediaRenderer:1",
    ] {
        assert!(
            requests
                .iter()
                .any(|r| r.contains(&format!("\r\nST: {target}\r\n"))),
            "{target}"
        );
    }
}

#[tokio::test]
async fn several_answers_for_one_receiver_give_one_result() {
    let http = avr_x_receiver().await;
    let answer = ssdp_answer(&location(&http));
    let ssdp = FakeSsdp::start(vec![answer.clone(), answer]).await;
    let found = discover(&config(&ssdp, &http)).await.unwrap();
    // Two answers for each of the three searches.
    assert_eq!(found.len(), 1, "{found:?}");
    // The description and the device information were fetched once.
    let descriptions = http
        .requests()
        .iter()
        .filter(|seen| seen.path == "/description.xml")
        .count();
    assert_eq!(descriptions, 1);
}

#[tokio::test]
async fn nothing_that_answers_gives_an_empty_list() {
    let http = avr_x_receiver().await;
    let ssdp = FakeSsdp::start(Vec::new()).await;
    assert_eq!(discover(&config(&ssdp, &http)).await.unwrap(), []);
}

#[tokio::test]
async fn datagrams_that_are_not_answers_are_ignored() {
    let http = avr_x_receiver().await;
    let answers = vec![
        b"garbage".to_vec(),
        vec![0xFF, 0xFE],
        b"HTTP/1.1 200 OK\r\nST: upnp:rootdevice\r\n\r\n".to_vec(),
        // A location that is not used: other scheme, public address, name.
        ssdp_answer("https://127.0.0.1/description.xml"),
        ssdp_answer("http://93.184.216.34/description.xml"),
        ssdp_answer("http://receiver.invalid/description.xml"),
        ssdp_answer("not a url"),
    ];
    let ssdp = FakeSsdp::start(answers).await;
    assert_eq!(discover(&config(&ssdp, &http)).await.unwrap(), []);
    assert!(http.requests().is_empty(), "{:?}", http.requests());
}

#[tokio::test]
async fn a_strict_policy_needs_the_address_of_the_sender() {
    let http = avr_x_receiver().await;
    // The answer comes from 127.0.0.1, the location is another loopback address.
    let other = format!("http://127.0.0.2:{}/description.xml", http.port);
    let ssdp = FakeSsdp::start(vec![ssdp_answer(&other)]).await;
    let strict = DiscoveryConfig {
        locations: LocationPolicy::SameHost,
        ..config(&ssdp, &http)
    };
    assert_eq!(discover(&strict).await.unwrap(), []);
    assert!(http.requests().is_empty(), "{:?}", http.requests());

    let same = FakeSsdp::start(vec![ssdp_answer(&location(&http))]).await;
    let strict = DiscoveryConfig {
        locations: LocationPolicy::SameHost,
        ..config(&same, &http)
    };
    assert_eq!(discover(&strict).await.unwrap().len(), 1);
}

#[tokio::test]
async fn other_devices_are_left_out() {
    // Another maker.
    let other_maker = Fake::start(|seen| match seen.path.as_str() {
        "/description.xml" => Reply::Body(
            b"<root xmlns=\"urn:schemas-upnp-org:device-1-0\"><device>\
              <deviceType>urn:schemas-upnp-org:device:MediaRenderer:1</deviceType>\
              <manufacturer>Sonos</manufacturer><modelName>Play:1</modelName>\
              <serialNumber>1</serialNumber><friendlyName>Kitchen</friendlyName>\
              </device></root>"
                .to_vec(),
        ),
        _ => Reply::Status(404),
    })
    .await;
    let ssdp = FakeSsdp::start(vec![ssdp_answer(&location(&other_maker))]).await;
    assert_eq!(discover(&config(&ssdp, &other_maker)).await.unwrap(), []);

    // A Denon device that says that it is not an AV receiver.
    let speaker = avr_x_receiver_with_info(SOUND_BAR_INFO).await;
    let ssdp = FakeSsdp::start(vec![ssdp_answer(&location(&speaker))]).await;
    assert_eq!(discover(&config(&ssdp, &speaker)).await.unwrap(), []);

    // A page that is not there: nothing to read.
    let missing = Fake::start(|_| Reply::Status(404)).await;
    let ssdp = FakeSsdp::start(vec![ssdp_answer(&location(&missing))]).await;
    assert_eq!(discover(&config(&ssdp, &missing)).await.unwrap(), []);
}

#[tokio::test]
async fn a_receiver_with_a_category_or_without_device_information_is_kept() {
    let with_category = avr_x_receiver_with_info(AV_RECEIVER_INFO).await;
    let ssdp = FakeSsdp::start(vec![ssdp_answer(&location(&with_category))]).await;
    assert_eq!(
        discover(&config(&ssdp, &with_category))
            .await
            .unwrap()
            .len(),
        1
    );

    // No answer to the device information: older receivers have none.
    let http = Fake::start(|seen| match seen.path.as_str() {
        "/description.xml" => Reply::Body(common::DESCRIPTION.to_vec()),
        _ => Reply::Status(404),
    })
    .await;
    let ssdp = FakeSsdp::start(vec![ssdp_answer(&location(&http))]).await;
    assert_eq!(discover(&config(&ssdp, &http)).await.unwrap().len(), 1);
}

#[tokio::test]
async fn an_address_that_cannot_send_is_skipped() {
    let http = avr_x_receiver().await;
    let ssdp = FakeSsdp::start(vec![ssdp_answer(&location(&http))]).await;
    // 203.0.113.1 is not an address of this computer.
    let bad = Ipv4Addr::new(203, 0, 113, 1);
    let mixed = DiscoveryConfig {
        interfaces: Some(vec![bad, LOOPBACK]),
        ..config(&ssdp, &http)
    };
    assert_eq!(discover(&mixed).await.unwrap().len(), 1);

    let only_bad = DiscoveryConfig {
        interfaces: Some(vec![bad]),
        ..config(&ssdp, &http)
    };
    let error = discover(&only_bad).await.unwrap_err();
    assert!(matches!(error, Error::Network { .. }), "{error:?}");
    assert!(error.is_network());
}

#[tokio::test]
async fn no_addresses_to_search_from_is_an_empty_list() {
    let http = avr_x_receiver().await;
    let ssdp = FakeSsdp::start(Vec::new()).await;
    let none = DiscoveryConfig {
        interfaces: Some(Vec::new()),
        ..config(&ssdp, &http)
    };
    assert_eq!(discover(&none).await.unwrap(), []);
    assert_eq!(ssdp.requests(), Vec::<String>::new());
}
