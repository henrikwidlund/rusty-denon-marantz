//! The fast start against a fake receiver, a fake web interface and a fake
//! telnet port.

#![cfg(feature = "telnet")]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{
    AVR_X_INFO, Fake, FakeTelnet, Reply, avr_x_receiver, builder, config, friendly_name_answer,
    is_friendly_name_request, wait_until,
};
use denon_marantz::{
    Avr, PageStatus, QueryGroup, ReceiverType, SkipReason, StartOptions, TelnetConfig, TelnetStart,
    Zone,
};
use denon_marantz_proto::telnet::{SignalPart, signal_queries};
use denon_marantz_proto::{AUDIO_CHANNELS_PATH, Brand, VIDEO_SIGNAL_PATH};

const VIDEO_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-video-signal.xml");
const AUDIO_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-audio-channels.xml");

const fn telnet_config(port: u16) -> TelnetConfig {
    TelnetConfig {
        port,
        connect_timeout: Duration::from_millis(500),
        write_timeout: Duration::from_millis(500),
        keep_alive: Duration::from_secs(10),
        reconnect_min: Duration::from_millis(50),
        reconnect_max: Duration::from_millis(100),
        command_interval: Duration::ZERO,
        query_interval: Duration::ZERO,
        query_quiet: Duration::ZERO,
        background_pause: Duration::ZERO,
        query_groups: QueryGroup::DEFAULT_ORDER,
    }
}

/// A web interface that has the signal pages.
async fn web_interface() -> Fake {
    Fake::start(|seen| match seen.path.as_str() {
        VIDEO_SIGNAL_PATH => Reply::Body(VIDEO_PAGE.to_vec()),
        AUDIO_CHANNELS_PATH => Reply::Body(AUDIO_PAGE.to_vec()),
        _ => Reply::Status(404),
    })
    .await
}

async fn started(web: &Fake, telnet: &FakeTelnet) -> Arc<Avr> {
    let http = avr_x_receiver().await;
    Arc::new(
        builder(&http, ReceiverType::AvrX)
            .web_interface_port(web.port)
            .web_interface_config(config())
            .telnet_config(telnet_config(telnet.port))
            .connect()
            .await
            .unwrap(),
    )
}

fn lines(received: &str) -> Vec<&str> {
    received
        .split('\r')
        .filter(|line| !line.is_empty())
        .collect()
}

#[tokio::test]
async fn the_state_is_ready_when_start_returns() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let started = avr.start().await.unwrap();
    // The AppCommand read is done.
    assert!(avr.snapshot().zone(Zone::Main).power.is_some());
    assert!(avr.snapshot().zone(Zone::Main).volume.is_some());

    let report = started.finished().await;
    assert!(matches!(report.telnet, TelnetStart::Open));
    let pages = report.web_interface.unwrap();
    assert_eq!(pages.video_signal, PageStatus::Read);
    assert_eq!(pages.audio_channels, PageStatus::Read);
    let state = avr.snapshot();
    assert_eq!(state.device().signal.video.input.as_deref(), Some("4K50"));
    assert!(state.device().signal.channels.output.is_some());
}

#[tokio::test]
async fn telnet_is_asked_first_for_what_http_does_not_answer() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    avr.start().await.unwrap().finished().await;
    wait_until("the queries", || {
        lines(&telnet.received()).contains(&"SSINFSIGECI ?")
    })
    .await;
    let received = telnet.received();
    let sent = lines(&received);
    assert_eq!(sent[0], "MSQUICK ?");
    // What AppCommand answers is not asked.
    for answered in ["MV?", "MU?", "SI?", "MS?"] {
        assert!(!sent.contains(&answered), "{answered}");
    }
    // What the signal pages answer is not asked, the audio signal is.
    assert!(sent.contains(&"SYSDA ?"));
    for text in signal_queries(Brand::Denon, SignalPart::Video)
        .chain(signal_queries(Brand::Denon, SignalPart::Channels))
    {
        assert!(!sent.contains(&text), "{text}");
    }
}

#[tokio::test]
async fn a_signal_page_that_fails_is_asked_on_telnet() {
    let web = Fake::start(|seen| match seen.path.as_str() {
        AUDIO_CHANNELS_PATH => Reply::Body(AUDIO_PAGE.to_vec()),
        _ => Reply::Status(404),
    })
    .await;
    let telnet = FakeTelnet::start().await;
    let avr = started(&web, &telnet).await;
    let report = avr.start().await.unwrap().finished().await;
    let pages = report.web_interface.unwrap();
    assert_eq!(pages.video_signal, PageStatus::NotFound);
    assert_eq!(pages.audio_channels, PageStatus::Read);
    wait_until("the video queries", || {
        lines(&telnet.received()).contains(&"SSINFSIGRES ?")
    })
    .await;
    let received = telnet.received();
    let sent = lines(&received);
    assert!(sent.contains(&"SYSDVIN ?"));
    assert!(!sent.contains(&"OPINFASP ?"));
}

#[tokio::test]
async fn a_page_with_a_server_error_is_asked_once_more() {
    let asked = Arc::new(AtomicUsize::new(0));
    let web = {
        let asked = asked.clone();
        Fake::start(move |seen| match seen.path.as_str() {
            VIDEO_SIGNAL_PATH if asked.fetch_add(1, Ordering::SeqCst) == 0 => Reply::Status(500),
            VIDEO_SIGNAL_PATH => Reply::Body(VIDEO_PAGE.to_vec()),
            AUDIO_CHANNELS_PATH => Reply::Body(AUDIO_PAGE.to_vec()),
            _ => Reply::Status(404),
        })
        .await
    };
    let telnet = FakeTelnet::start().await;
    let avr = started(&web, &telnet).await;
    let report = avr.start().await.unwrap().finished().await;
    assert_eq!(report.web_interface.unwrap().video_signal, PageStatus::Read);
    assert_eq!(asked.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_missing_page_is_not_asked_again() {
    let asked = Arc::new(AtomicUsize::new(0));
    let web = {
        let asked = asked.clone();
        Fake::start(move |seen| {
            if seen.path == VIDEO_SIGNAL_PATH {
                asked.fetch_add(1, Ordering::SeqCst);
            }
            Reply::Status(404)
        })
        .await
    };
    let telnet = FakeTelnet::start().await;
    let avr = started(&web, &telnet).await;
    avr.start().await.unwrap().finished().await;
    assert_eq!(asked.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn the_signal_pages_can_be_turned_off() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let report = avr
        .start_with(StartOptions::new().signal_pages(false))
        .await
        .unwrap()
        .finished()
        .await;
    let pages = report.web_interface.unwrap();
    assert_eq!(
        pages.video_signal,
        PageStatus::Skipped(SkipReason::NotAsked)
    );
    assert!(
        web.requests()
            .iter()
            .all(|seen| seen.path != VIDEO_SIGNAL_PATH && seen.path != AUDIO_CHANNELS_PATH)
    );
    // Telnet gives the signal instead.
    wait_until("the video queries", || {
        lines(&telnet.received()).contains(&"SSINFSIGRES ?")
    })
    .await;
}

#[tokio::test]
async fn a_closed_telnet_port_is_in_the_report() {
    let web = web_interface().await;
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let http = avr_x_receiver().await;
    let avr = Arc::new(
        builder(&http, ReceiverType::AvrX)
            .web_interface_port(web.port)
            .web_interface_config(config())
            .telnet_config(telnet_config(port))
            .connect()
            .await
            .unwrap(),
    );
    let report = avr.start().await.unwrap().finished().await;
    assert!(matches!(report.telnet, TelnetStart::Failed(_)));
    // The pages were read anyway.
    assert!(report.web_interface.unwrap().video_signal.is_read());
}

#[tokio::test]
async fn telnet_can_be_left_closed() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let report = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap()
        .finished()
        .await;
    assert!(matches!(report.telnet, TelnetStart::Disabled));
    assert_eq!(telnet.connections(), 0);
}

#[tokio::test]
async fn a_web_interface_that_does_not_answer_is_in_the_report() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver().await;
    let avr = Arc::new(
        builder(&http, ReceiverType::AvrX)
            .web_interface_port(port)
            .web_interface_config(config())
            .telnet_config(telnet_config(telnet.port))
            .connect()
            .await
            .unwrap(),
    );
    let report = avr.start().await.unwrap().finished().await;
    assert!(report.web_interface.unwrap_err().is_unreachable());
    // Telnet gives the signal then.
    wait_until("the signal queries", || {
        let received = telnet.received();
        let sent = lines(&received);
        sent.contains(&"SSINFSIGRES ?") && sent.contains(&"OPINFASP ?")
    })
    .await;
}

#[tokio::test]
async fn refresh_signal_reads_the_pages_and_asks_telnet_for_the_audio() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    avr.start().await.unwrap().finished().await;
    wait_until("the first queries", || {
        lines(&telnet.received()).contains(&"SSINFSIGECI ?")
    })
    .await;
    let before = telnet.received().len();
    avr.refresh_signal().await.unwrap();
    wait_until("the audio queries", || {
        telnet.received().len() > before + 40
    })
    .await;
    let received = telnet.received();
    let sent = lines(&received[before..]);
    assert!(sent.contains(&"SYSDA ?"));
    assert!(sent.contains(&"SSINFSIGECI ?"));
    // The pages were read, so telnet is not asked for these.
    assert!(!sent.contains(&"SSINFSIGRES ?"));
    assert!(!sent.contains(&"OPINFASP ?"));
}

#[tokio::test]
async fn refresh_signal_without_telnet_reads_only_the_pages() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    avr.refresh_signal().await.unwrap();
    assert!(avr.snapshot().device().signal.channels.input.is_some());
    assert_eq!(telnet.connections(), 0);
}

#[tokio::test]
async fn refresh_signal_is_an_error_if_nothing_can_be_asked() {
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .web_interface_port(port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    assert!(avr.refresh_signal().await.unwrap_err().is_unreachable());
}

#[test]
fn the_default_options_are_the_new_ones() {
    assert_eq!(StartOptions::default(), StartOptions::new());
}

#[tokio::test]
async fn a_failed_read_ends_the_start_and_closes_telnet() {
    let http = Fake::start(|seen| match (seen.method.as_str(), seen.path.as_str()) {
        ("GET", "/goform/Deviceinfo.xml") => Reply::Body(AVR_X_INFO.to_vec()),
        ("POST", "/goform/AppCommand.xml") if is_friendly_name_request(&seen.body) => {
            Reply::Body(friendly_name_answer("X"))
        }
        ("POST", _) => Reply::Body(b"nothing".to_vec()),
        _ => Reply::Status(404),
    })
    .await;
    let telnet = FakeTelnet::start().await;
    let avr = Arc::new(
        builder(&http, ReceiverType::AvrX)
            .telnet_config(telnet_config(telnet.port))
            .connect()
            .await
            .unwrap(),
    );
    assert!(avr.start().await.is_err());
    wait_until("telnet closed", || telnet.closed_by_client() == 1).await;
}

#[tokio::test]
async fn a_page_that_does_not_answer_is_only_that_page() {
    let web = Fake::start(|seen| match seen.path.as_str() {
        VIDEO_SIGNAL_PATH => Reply::Hang,
        AUDIO_CHANNELS_PATH => Reply::Body(AUDIO_PAGE.to_vec()),
        _ => Reply::Status(404),
    })
    .await;
    let telnet = FakeTelnet::start().await;
    let avr = started(&web, &telnet).await;
    let report = avr.start().await.unwrap().finished().await;
    // The web interface answered, so the report has the pages that were read.
    let pages = report.web_interface.unwrap();
    assert_eq!(pages.availability.pages.len(), 7);
    assert_eq!(pages.video_signal, PageStatus::NoAnswer);
    assert_eq!(pages.audio_channels, PageStatus::Read);
    // Telnet is asked for the video values that the page did not give.
    wait_until("the video queries", || {
        lines(&telnet.received()).contains(&"SSINFSIGRES ?")
    })
    .await;
}

#[tokio::test]
async fn a_failed_start_keeps_a_telnet_connection_that_it_did_not_open() {
    let http = Fake::start(|seen| match (seen.method.as_str(), seen.path.as_str()) {
        ("GET", "/goform/Deviceinfo.xml") => Reply::Body(AVR_X_INFO.to_vec()),
        ("POST", "/goform/AppCommand.xml") if is_friendly_name_request(&seen.body) => {
            Reply::Body(friendly_name_answer("X"))
        }
        ("POST", _) => Reply::Body(b"nothing".to_vec()),
        _ => Reply::Status(404),
    })
    .await;
    let telnet = FakeTelnet::start().await;
    let avr = Arc::new(
        builder(&http, ReceiverType::AvrX)
            .telnet_config(telnet_config(telnet.port))
            .connect()
            .await
            .unwrap(),
    );
    avr.telnet_connect().await.unwrap();
    assert!(avr.start().await.is_err());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(telnet.closed_by_client(), 0);
    assert_eq!(
        *avr.telnet_status().borrow(),
        denon_marantz::TelnetStatus::Connected
    );
}

#[tokio::test]
async fn refresh_signal_says_when_nothing_was_read() {
    let good = Arc::new(std::sync::atomic::AtomicBool::new(true));
    let web = {
        let good = good.clone();
        Fake::start(move |seen| match seen.path.as_str() {
            VIDEO_SIGNAL_PATH if good.load(Ordering::SeqCst) => Reply::Body(VIDEO_PAGE.to_vec()),
            AUDIO_CHANNELS_PATH if good.load(Ordering::SeqCst) => Reply::Body(AUDIO_PAGE.to_vec()),
            // Valid XML, but not the page.
            VIDEO_SIGNAL_PATH | AUDIO_CHANNELS_PATH => Reply::Body(b"<x/>".to_vec()),
            _ => Reply::Status(404),
        })
        .await
    };
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .web_interface_port(web.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    avr.refresh_signal().await.unwrap();
    assert!(avr.snapshot().device().signal.video.input.is_some());

    good.store(false, Ordering::SeqCst);
    let error = avr.refresh_signal().await.unwrap_err();
    assert!(
        matches!(error, denon_marantz::Error::NothingRead(_)),
        "{error:?}"
    );
    // The other page did not clear what was known.
    assert_eq!(
        avr.snapshot().device().signal.video.input.as_deref(),
        Some("4K50")
    );
    assert!(avr.snapshot().device().signal.channels.input.is_some());
}
