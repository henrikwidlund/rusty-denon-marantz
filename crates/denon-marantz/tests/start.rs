//! The fast start against a fake receiver, a fake web interface and a fake
//! telnet port.

#![cfg(feature = "telnet")]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use tokio::time::timeout;

use common::{
    AVR_X_INFO, Fake, FakeTelnet, Reply, avr_x_receiver, builder, config, friendly_name_answer,
    is_friendly_name_request, wait_until,
};
use denon_marantz::TelnetStatus;
use denon_marantz::{
    Avr, Error, PageStatus, QueryGroup, ReceiverType, SkipReason, StartOptions, TelnetConfig,
    TelnetStart, TelnetSync, UpdateMethod, Zone,
};
use denon_marantz_proto::telnet::{SignalPart, group_queries, signal_queries};
use denon_marantz_proto::{AUDIO_CHANNELS_PATH, Brand, VIDEO_SIGNAL_PATH};

const VIDEO_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-video-signal.xml");
const AUDIO_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-audio-channels.xml");

const OLD_STATUS: &[u8] = include_bytes!(
    "../../denon-marantz-proto/tests/data/xml/AVR-3313-formMainZone_MainZoneXmlStatus.xml"
);
const OLD_ZONE_PAGE: &[u8] = include_bytes!(
    "../../denon-marantz-proto/tests/data/xml/AVR-3313-formMainZone_MainZoneXml.xml"
);

/// The groups of the start and the Device group, which the start does not ask for.
const WITH_DEVICE: &[QueryGroup] = &[
    QueryGroup::Core,
    QueryGroup::Names,
    QueryGroup::Toggles,
    QueryGroup::Signal,
    QueryGroup::Device,
];

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
        query_groups: QueryGroup::START_ORDER,
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
    connected(
        &http,
        ReceiverType::AvrX,
        web,
        telnet,
        QueryGroup::START_ORDER,
    )
    .await
}

async fn connected(
    http: &Fake,
    receiver_type: ReceiverType,
    web: &Fake,
    telnet: &FakeTelnet,
    query_groups: &'static [QueryGroup],
) -> Arc<Avr> {
    Arc::new(
        builder(http, receiver_type)
            .web_interface_port(web.port)
            .web_interface_config(config())
            .telnet_config(TelnetConfig {
                query_groups,
                ..telnet_config(telnet.port)
            })
            .connect()
            .await
            .unwrap(),
    )
}

/// Waits until the queries that wait in the connection are written. A group
/// that the start does not ask for goes last, so its last query shows the end.
async fn wait_for_the_queue(avr: &Avr, telnet: &FakeTelnet) {
    avr.telnet_refresh_group(QueryGroup::Zones).await.unwrap();
    let last = group_queries(Brand::Denon, QueryGroup::Zones)
        .last()
        .unwrap();
    wait_until("the end of the queue", || {
        lines(&telnet.received()).contains(&last)
    })
    .await;
}

fn lines(received: &str) -> Vec<&str> {
    received
        .split('\r')
        .filter(|line| !line.is_empty())
        .collect()
}

#[tokio::test]
async fn all_synced_waits_for_the_answers_to_the_telnet_queries() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let started = avr.start().await.unwrap();
    let waiting = tokio::spawn(started.all_synced());
    // The receiver does not answer yet: the last line is the marker query,
    // after all queries of the start.
    wait_until("the marker", || {
        let received = telnet.received();
        let sent = lines(&received);
        sent.contains(&"SSINFSIGECI ?") && sent.last() == Some(&"PW?")
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!waiting.is_finished());
    // The answer to an earlier query comes first, then the marker's.
    telnet.send(b"MV12\rPWON\r");
    let report = timeout(Duration::from_secs(5), waiting)
        .await
        .expect("all_synced did not end")
        .unwrap();
    assert_eq!(report.telnet_sync, TelnetSync::Answered);
    assert!(matches!(report.telnet, TelnetStart::Open));
    let volume = avr.snapshot().zone(Zone::Main).volume;
    assert_eq!(volume.map(denon_marantz::Volume::db), Some(-68.0));
}

#[tokio::test]
async fn a_telnet_sync_that_is_dropped_does_not_stop_the_next_one() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    avr.start().await.unwrap().basic_synced().await;
    // The first call is dropped at once, before or after its marker is written.
    let _ = timeout(Duration::from_millis(1), avr.telnet_sync()).await;
    let second = {
        let avr = avr.clone();
        tokio::spawn(async move { avr.telnet_sync().await })
    };
    // An answer for a marker that was written and dropped may come first.
    for _ in 0..50 {
        if second.is_finished() {
            break;
        }
        telnet.send(b"PWON\r");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    timeout(Duration::from_secs(1), second)
        .await
        .expect("the second call did not end")
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn each_telnet_sync_waits_for_its_own_answer() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    avr.start().await.unwrap().basic_synced().await;
    let (first, second) = (avr.clone(), avr.clone());
    let first = tokio::spawn(async move { first.telnet_sync().await });
    let second = tokio::spawn(async move { second.telnet_sync().await });
    wait_until("two markers", || {
        lines(&telnet.received())
            .iter()
            .filter(|line| **line == "PW?")
            .count()
            >= 2
    })
    .await;
    telnet.send(b"PWON\r");
    timeout(Duration::from_secs(5), first)
        .await
        .expect("the first did not end")
        .unwrap()
        .unwrap();
    assert!(!second.is_finished());
    telnet.send(b"PWON\r");
    timeout(Duration::from_secs(5), second)
        .await
        .expect("the second did not end")
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn all_synced_reports_a_connection_that_ends_before_the_answers() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let started = avr.start().await.unwrap();
    let waiting = tokio::spawn(started.all_synced());
    wait_until("the marker", || {
        lines(&telnet.received()).last() == Some(&"PW?")
    })
    .await;
    telnet.close();
    let report = timeout(Duration::from_secs(5), waiting)
        .await
        .expect("all_synced did not end")
        .unwrap();
    assert_eq!(report.telnet_sync, TelnetSync::Lost);
}

#[tokio::test]
async fn all_synced_ends_with_no_answer_after_the_wait() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let started = avr
        .start_with(StartOptions::new().telnet_wait(Duration::from_millis(300)))
        .await
        .unwrap();
    // The fake telnet port never answers the last query.
    let report = timeout(Duration::from_secs(5), started.all_synced())
        .await
        .expect("all_synced did not end");
    assert_eq!(report.telnet_sync, TelnetSync::NoAnswer);
    assert!(matches!(report.telnet, TelnetStart::Open));
}

#[tokio::test]
async fn the_groups_of_the_config_are_asked_at_the_start_after_the_first_queries() {
    const GROUPS: &[QueryGroup] = &[
        QueryGroup::Core,
        QueryGroup::Names,
        QueryGroup::Toggles,
        QueryGroup::Signal,
        QueryGroup::Device,
        QueryGroup::Device,
    ];
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let http = avr_x_receiver().await;
    let avr = connected(&http, ReceiverType::AvrX, &web, &telnet, GROUPS).await;
    avr.start().await.unwrap().basic_synced().await;
    wait_for_the_queue(&avr, &telnet).await;
    let received = telnet.received();
    let sent = lines(&received);
    let device: Vec<_> = group_queries(Brand::Denon, QueryGroup::Device).collect();
    // The last query of the first list: the signal queries that only telnet answers.
    let last_first = signal_queries(Brand::Denon, SignalPart::TelnetOnly)
        .last()
        .unwrap();
    let after = sent.iter().position(|line| *line == last_first).unwrap();
    for query in &device {
        // A group that is in the config twice is asked once.
        let count = sent.iter().filter(|line| *line == query).count();
        assert_eq!(count, 1, "{query}");
        assert!(sent.iter().position(|line| line == query).unwrap() > after);
    }
    // The groups that are not in the config stay out.
    let picture = group_queries(Brand::Denon, QueryGroup::Picture)
        .next()
        .unwrap();
    assert!(!sent.contains(&picture));
}

/// How often the client sent each query of the group.
fn asked(sent: &[&str], group: QueryGroup) -> Vec<usize> {
    group_queries(Brand::Denon, group)
        .map(|query| sent.iter().filter(|line| **line == query).count())
        .collect()
}

#[tokio::test]
async fn a_receiver_without_appcommand_asks_the_groups_of_the_config_once() {
    let http = Fake::start(|seen| {
        let path = seen.path.split('?').next().unwrap_or_default();
        match path {
            "/goform/Deviceinfo.xml" => Reply::Body(AVR_X_INFO.to_vec()),
            "/goform/formMainZone_MainZoneXmlStatus.xml" => Reply::Body(OLD_STATUS.to_vec()),
            "/goform/formMainZone_MainZoneXml.xml" => Reply::Body(OLD_ZONE_PAGE.to_vec()),
            _ => Reply::Status(404),
        }
    })
    .await;
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = connected(&http, ReceiverType::AvrX, &web, &telnet, WITH_DEVICE).await;
    assert_eq!(avr.info().update_method, UpdateMethod::StatusPages);
    let report = avr.start().await.unwrap().basic_synced().await;
    assert!(matches!(report.telnet, TelnetStart::Open));
    wait_for_the_queue(&avr, &telnet).await;
    let received = telnet.received();
    let sent = lines(&received);
    assert!(
        asked(&sent, QueryGroup::Device)
            .iter()
            .all(|count| *count == 1)
    );
    assert!(
        asked(&sent, QueryGroup::Picture)
            .iter()
            .all(|count| *count == 0)
    );
}

#[tokio::test]
async fn a_reconnect_asks_the_groups_of_the_config_again_once() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let http = avr_x_receiver().await;
    let avr = connected(&http, ReceiverType::AvrX, &web, &telnet, WITH_DEVICE).await;
    avr.start().await.unwrap().basic_synced().await;
    wait_for_the_queue(&avr, &telnet).await;
    let last = group_queries(Brand::Denon, QueryGroup::Device)
        .last()
        .unwrap();
    let count = |received: &str| lines(received).iter().filter(|line| **line == last).count();
    assert_eq!(count(&telnet.received()), 1);

    telnet.close();
    // The last query of the group is the end of the second list.
    wait_until("the queries after the reconnect", || {
        count(&telnet.received()) == 2
    })
    .await;
    assert_eq!(telnet.connections(), 2);
    let received = telnet.received();
    let sent = lines(&received);
    assert!(
        asked(&sent, QueryGroup::Device)
            .iter()
            .all(|count| *count == 2)
    );
}

#[tokio::test]
async fn a_start_with_a_running_connection_does_not_ask_the_groups_again() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let http = avr_x_receiver().await;
    let avr = connected(&http, ReceiverType::AvrX, &web, &telnet, WITH_DEVICE).await;
    avr.telnet_connect().await.unwrap();
    wait_for_the_queue(&avr, &telnet).await;
    let received = telnet.received();
    assert!(
        asked(&lines(&received), QueryGroup::Device)
            .iter()
            .all(|count| *count == 1)
    );

    let report = avr.start().await.unwrap().basic_synced().await;
    assert!(matches!(report.telnet, TelnetStart::AlreadyRunning));
    // A query that is sent after the start ends the wait for the queue.
    wait_for_the_queue(&avr, &telnet).await;
    let received = telnet.received();
    assert!(
        asked(&lines(&received), QueryGroup::Device)
            .iter()
            .all(|count| *count == 1)
    );
    assert_eq!(telnet.connections(), 1);
}

#[tokio::test]
async fn all_synced_without_telnet_does_not_wait() {
    let web = web_interface().await;
    let http = avr_x_receiver().await;
    let avr = Arc::new(
        builder(&http, ReceiverType::AvrX)
            .web_interface_port(web.port)
            .web_interface_config(config())
            .connect()
            .await
            .unwrap(),
    );
    let report = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap()
        .all_synced()
        .await;
    assert_eq!(report.telnet_sync, TelnetSync::NotOpen);
}

#[tokio::test]
async fn the_start_asks_for_each_query_once() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    avr.start().await.unwrap().basic_synced().await;
    wait_for_the_queue(&avr, &telnet).await;
    let received = telnet.received();
    let sent = lines(&received);
    let zones = group_queries(Brand::Denon, QueryGroup::Zones).count();
    let mut asked: Vec<&str> = sent[..sent.len() - zones]
        .iter()
        .copied()
        .filter(|line| line.ends_with('?'))
        .collect();
    let total = asked.len();
    asked.sort_unstable();
    asked.dedup();
    assert_eq!(asked.len(), total, "a query was sent twice");
}

#[tokio::test]
async fn the_state_is_ready_when_start_returns() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let started = avr.start().await.unwrap();
    // The AppCommand read is done.
    assert!(avr.snapshot().zone(Zone::Main).power.is_some());
    assert!(avr.snapshot().zone(Zone::Main).volume.is_some());

    let report = started.basic_synced().await;
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
    avr.start().await.unwrap().basic_synced().await;
    wait_for_the_queue(&avr, &telnet).await;
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
    let report = avr.start().await.unwrap().basic_synced().await;
    let pages = report.web_interface.unwrap();
    assert_eq!(pages.video_signal, PageStatus::NotFound);
    assert_eq!(pages.audio_channels, PageStatus::Read);
    wait_for_the_queue(&avr, &telnet).await;
    let received = telnet.received();
    let sent = lines(&received);
    assert!(sent.contains(&"SSINFSIGRES ?"));
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
    let report = avr.start().await.unwrap().basic_synced().await;
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
    avr.start().await.unwrap().basic_synced().await;
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
        .basic_synced()
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
    let report = avr.start().await.unwrap().basic_synced().await;
    assert!(
        matches!(report.telnet, TelnetStart::Failed(_)),
        "{:?}",
        report.telnet
    );
    // The pages were read anyway.
    assert!(report.web_interface.unwrap().video_signal.is_read());
}

#[tokio::test]
async fn a_start_that_is_dropped_leaves_telnet_closed() {
    let hang = Arc::new(AtomicBool::new(false));
    let http = common::avr_x_receiver_that_can_hang(hang.clone()).await;
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = Arc::new(
        builder(&http, ReceiverType::AvrX)
            .web_interface_port(web.port)
            .web_interface_config(config())
            .telnet_config(telnet_config(telnet.port))
            .connect()
            .await
            .unwrap(),
    );
    // The zones are not read in time, so the start is dropped.
    hang.store(true, Ordering::SeqCst);
    let late = timeout(Duration::from_millis(100), avr.start()).await;
    assert!(late.is_err());
    assert_eq!(telnet.connections(), 0);
    assert_eq!(*avr.telnet_status().borrow(), TelnetStatus::Disconnected);

    // Another start works, and its queries are the ones of a first start.
    hang.store(false, Ordering::SeqCst);
    let report = avr.start().await.unwrap().basic_synced().await;
    assert!(matches!(report.telnet, TelnetStart::Open));
    wait_until("the first query", || {
        lines(&telnet.received()).first() == Some(&"MSQUICK ?")
    })
    .await;
}

#[tokio::test]
async fn a_second_start_is_refused_while_the_first_runs() {
    let hang = Arc::new(AtomicBool::new(false));
    let http = common::avr_x_receiver_that_can_hang(hang.clone()).await;
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = Arc::new(
        builder(&http, ReceiverType::AvrX)
            .web_interface_port(web.port)
            .web_interface_config(config())
            .telnet_config(telnet_config(telnet.port))
            .connect()
            .await
            .unwrap(),
    );
    hang.store(true, Ordering::SeqCst);
    let known = http.requests().len();
    let first = tokio::spawn({
        let avr = avr.clone();
        async move { avr.start().await }
    });
    // The first start waits for the zones, so it holds the flag.
    wait_until("the zones request", || http.requests().len() > known).await;
    let before = http.requests().len();
    let second = avr.start().await.unwrap_err();
    assert!(matches!(second, Error::StartRunning), "{second}");
    // The refused start sent no request.
    assert_eq!(http.requests().len(), before);
    assert_eq!(telnet.connections(), 0);

    // A stopped start lets the next one run, also after a finished one.
    first.abort();
    let _ = first.await;
    hang.store(false, Ordering::SeqCst);
    avr.start().await.unwrap().basic_synced().await;
    avr.start().await.unwrap().basic_synced().await;
}

#[tokio::test]
async fn telnet_can_be_left_closed() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let report = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap()
        .basic_synced()
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
    let report = avr.start().await.unwrap().basic_synced().await;
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
    avr.start().await.unwrap().basic_synced().await;
    // The start asks for these too: wait for them, so that a second one
    // comes from the refresh.
    let count = |text: &str| {
        lines(&telnet.received())
            .iter()
            .filter(|line| **line == text)
            .count()
    };
    wait_until("the first queries", || {
        count("SSINFSIGECI ?") > 0 && count("SYSDA ?") > 0
    })
    .await;
    let before = telnet.received().len();
    let (eci, audio) = (count("SSINFSIGECI ?"), count("SYSDA ?"));
    avr.refresh_signal().await.unwrap();
    wait_until("the audio queries", || {
        count("SSINFSIGECI ?") > eci && count("SYSDA ?") > audio
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
async fn a_failed_read_ends_the_start_and_telnet_stays_closed() {
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
    assert_eq!(telnet.connections(), 0);
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
    let report = avr.start().await.unwrap().basic_synced().await;
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
    assert_eq!(*avr.telnet_status().borrow(), TelnetStatus::Connected);
}

#[tokio::test]
async fn refresh_signal_says_when_nothing_was_read() {
    let good = Arc::new(AtomicBool::new(true));
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
    assert!(matches!(error, Error::NothingRead), "{error:?}");
    // The other page did not clear what was known.
    assert_eq!(
        avr.snapshot().device().signal.video.input.as_deref(),
        Some("4K50")
    );
    assert!(avr.snapshot().device().signal.channels.input.is_some());
}

#[tokio::test]
async fn the_channels_page_is_not_read_when_appcommand_gave_the_channels() {
    let asked_channels = Arc::new(AtomicUsize::new(0));
    let web = {
        let asked_channels = asked_channels.clone();
        Fake::start(move |seen| match seen.path.as_str() {
            VIDEO_SIGNAL_PATH => Reply::Body(VIDEO_PAGE.to_vec()),
            AUDIO_CHANNELS_PATH => {
                asked_channels.fetch_add(1, Ordering::SeqCst);
                // A page that has no channels for the input would clear them.
                Reply::Body(b"<Audio><InputSignal mode=\"text_only\">DOLBY ATMOS</InputSignal><ActiveSpeaker><List><Speaker index=\"0\">2</Speaker></List></ActiveSpeaker></Audio>".to_vec())
            }
            _ => Reply::Status(404),
        })
        .await
    };
    let answer: &'static str = Box::leak(common::appcommand0300_answer("").into_boxed_str());
    let receiver = common::receiver_with_audyssey(Some(answer), true).await;
    let telnet = FakeTelnet::start().await;
    let avr = Arc::new(
        builder(&receiver, ReceiverType::AvrX2016)
            .web_interface_port(web.port)
            .web_interface_config(config())
            .telnet_config(telnet_config(telnet.port))
            .connect()
            .await
            .unwrap(),
    );
    let report = avr.start().await.unwrap().basic_synced().await;
    let pages = report.web_interface.unwrap();
    assert_eq!(
        pages.audio_channels,
        PageStatus::Skipped(SkipReason::AlreadyRead)
    );
    assert!(pages.audio_channels.has_values());
    assert_eq!(asked_channels.load(Ordering::SeqCst), 0);
    // The channels of the update stay.
    let state = avr.snapshot();
    assert_eq!(
        state.device().signal.channels.input.unwrap().to_string(),
        "2.0"
    );
    // Telnet is not asked for the channels either.
    wait_for_the_queue(&avr, &telnet).await;
    assert!(!telnet.received().contains("OPINFINS ?"));
}

#[tokio::test]
async fn refresh_signal_uses_appcommand_for_the_channels() {
    let answer: &'static str = Box::leak(common::appcommand0300_answer("").into_boxed_str());
    let receiver = common::receiver_with_audyssey(Some(answer), true).await;
    let web = web_interface().await;
    let avr = builder(&receiver, ReceiverType::AvrX2016)
        .web_interface_port(web.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    avr.refresh_signal().await.unwrap();
    let state = avr.snapshot();
    assert_eq!(
        state.device().signal.audio_signal.as_deref(),
        Some("Dolby Atmos")
    );
    assert_eq!(
        state.device().signal.channels.output.unwrap().to_string(),
        "2.1.1"
    );
    assert!(
        web.requests()
            .iter()
            .all(|seen| seen.path != AUDIO_CHANNELS_PATH)
    );
    // The video page still gives what AppCommand does not have.
    assert!(
        web.requests()
            .iter()
            .any(|seen| seen.path == VIDEO_SIGNAL_PATH)
    );
}

async fn start_2016_receiver(answer: Option<&'static str>) -> FakeTelnet {
    let web = web_interface().await;
    let receiver = common::receiver_with_audyssey(answer, true).await;
    let telnet = FakeTelnet::start().await;
    let avr = Arc::new(
        builder(&receiver, ReceiverType::AvrX2016)
            .web_interface_port(web.port)
            .web_interface_config(config())
            .telnet_config(telnet_config(telnet.port))
            .connect()
            .await
            .unwrap(),
    );
    avr.start().await.unwrap().basic_synced().await;
    // The queries of the fallback come after the first ones.
    wait_for_the_queue(&avr, &telnet).await;
    telnet
}

#[tokio::test]
async fn a_receiver_with_appcommand_is_not_asked_for_the_audio_values_over_telnet() {
    let answer: &'static str = Box::leak(common::appcommand0300_answer("").into_boxed_str());
    let telnet = start_2016_receiver(Some(answer)).await;
    let received = telnet.received();
    let sent = lines(&received);
    // The receiver pushes changes by itself, so these were only for the first
    // value, and AppCommand gave it.
    for query in ["SYSDA ?", "SYSMI ?", "SSINFAISFSV ?"] {
        assert!(!sent.contains(&query), "{query}");
    }
    // What only telnet answers is still asked.
    assert!(sent.contains(&"SYHDMIDIAGMAXRES ?"));
    assert!(sent.contains(&"SSINFSIGECI ?"));
}

#[tokio::test]
async fn a_short_appcommand_answer_gets_the_audio_queries_later() {
    // A busy receiver answers only the video item.
    let short = format!("<rx>{}</rx>", common::appcommand0300_item("GetVideoInfo"));
    let telnet = start_2016_receiver(Some(Box::leak(short.into_boxed_str()))).await;
    let received = telnet.received();
    let sent = lines(&received);
    for query in ["SYSDA ?", "SYSMI ?", "SSINFAISFSV ?"] {
        assert!(sent.contains(&query), "{query}");
    }
}

#[tokio::test]
async fn a_receiver_that_does_not_answer_appcommand_gets_the_audio_queries_later() {
    let telnet = start_2016_receiver(None).await;
    let received = telnet.received();
    let sent = lines(&received);
    for query in ["SYSDA ?", "SYSMI ?", "SSINFAISFSV ?"] {
        assert!(sent.contains(&query), "{query}");
    }
}
