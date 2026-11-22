//! The fast start against a fake receiver, a fake web interface and a fake
//! telnet port.

#![cfg(feature = "telnet")]

mod common;

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::task::JoinHandle;
use tokio::time::timeout;

use common::{
    AVR_X_INFO, AVR_X_OUTPUT_SETTINGS_INFO, AVR_X_SETTINGS_AND_SIGNAL_INFO, AVR_X_SETTINGS_INFO,
    Fake, FakeTelnet, OLDER_SETTINGS_INFO, Reply, SettingsReplies, appcommand0300_requests,
    avr_x_receiver, avr_x_receiver_that_can_hang, avr_x_receiver_with_held_settings,
    avr_x_receiver_with_info, avr_x_receiver_with_settings, builder, config, fake_group_queries,
    friendly_name_answer, is_friendly_name_request, wait_until,
};
use denon_marantz::TelnetStatus;
use denon_marantz::{
    Ask, Avr, Error, HttpReads, InfoCheck, MissingSources, PageStatus, Query, QueryGroup,
    ReceiverType, SkipReason, StartOptions, Started, TelnetConfig, TelnetStart, TelnetSync,
    UpdateMethod, Zone,
};
use denon_marantz_proto::telnet::{SignalPart, answer_prefix, group_queries, signal_queries};
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

/// No query, to compare with a list of queries.
const NONE: [&str; 0] = [];

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
        // Long, so that a wait does not end while a test answers.
        answer_quiet: Duration::from_secs(5),
        signal_wait_min: Duration::from_millis(300),
        signal_wait_max: Duration::from_millis(1200),
        signal_check: None,
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
    started_with(web, telnet_config(telnet.port)).await
}

/// Like [`started`], with these telnet settings.
async fn started_with(web: &Fake, telnet: TelnetConfig) -> Arc<Avr> {
    let http = avr_x_receiver().await;
    connected_with(&http, ReceiverType::AvrX, web, telnet).await
}

async fn connected(
    http: &Fake,
    receiver_type: ReceiverType,
    web: &Fake,
    telnet: &FakeTelnet,
    query_groups: &'static [QueryGroup],
) -> Arc<Avr> {
    let telnet = TelnetConfig {
        query_groups,
        ..telnet_config(telnet.port)
    };
    connected_with(http, receiver_type, web, telnet).await
}

async fn connected_with(
    http: &Fake,
    receiver_type: ReceiverType,
    web: &Fake,
    telnet: TelnetConfig,
) -> Arc<Avr> {
    Arc::new(
        builder(http, receiver_type)
            .web_interface_port(web.port)
            .web_interface_config(config())
            .telnet_config(telnet)
            .connect()
            .await
            .unwrap(),
    )
}

#[tokio::test]
async fn a_connection_to_a_receiver_with_one_zone_sends_no_zone_2_or_3_queries() {
    const ONE_ZONE_INFO: &[u8] = b"<Device_Info><CommApiVers>0301</CommApiVers>\
        <DeviceZones>1</DeviceZones></Device_Info>";
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let http = avr_x_receiver_with_info(ONE_ZONE_INFO).await;
    let avr = connected_with(&http, ReceiverType::AvrX, &web, telnet_config(telnet.port)).await;
    avr.telnet_connect().await.unwrap();
    avr.telnet_refresh_group(QueryGroup::Zones).await.unwrap();
    // A main zone group that the connect does not ask for goes last.
    avr.telnet_refresh_group(QueryGroup::Picture).await.unwrap();
    let last = group_queries(Brand::Denon, QueryGroup::Picture)
        .last()
        .unwrap();
    wait_until("the end of the queue", || {
        lines(&telnet.received()).contains(&last)
    })
    .await;
    let received = telnet.received();
    let sent = lines(&received);
    assert!(sent.contains(&"PW?") && !sent.contains(&"ZM?"), "{sent:?}");
    for line in sent.iter().filter(|line| line.ends_with('?')) {
        assert!(!line.contains("Z2") && !line.contains("Z3"), "{line}");
    }
}

/// Waits until the queries that wait in the connection are written. A group
/// that the start does not ask for goes last, so its last query shows the end.
async fn wait_for_the_queue(avr: &Avr, telnet: &FakeTelnet) {
    avr.telnet_refresh_group(QueryGroup::Zones).await.unwrap();
    let last = fake_group_queries(QueryGroup::Zones).last().unwrap();
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

/// Answers the queries that the client wrote since the last call, except the
/// ones in `hold`. Each answer has an end line, as a list has.
fn answer_new(telnet: &FakeTelnet, answered: &mut usize, hold: &[&str]) {
    let received = telnet.received();
    let sent = lines(&received);
    let mut reply = String::new();
    for query in sent.iter().skip(*answered) {
        if !hold.contains(query) {
            let prefix = answer_prefix(query);
            let _ = write!(reply, "{prefix} 1\r{prefix} END\r");
        }
    }
    *answered = sent.len();
    if !reply.is_empty() {
        telnet.send(reply.as_bytes());
    }
}

/// Answers the queries as [`answer_new`] does until `waiting` ends.
async fn answer_until_done<T>(telnet: &FakeTelnet, waiting: &JoinHandle<T>, hold: &[&str]) {
    let mut answered = 0;
    for _ in 0..200 {
        if waiting.is_finished() {
            return;
        }
        answer_new(telnet, &mut answered, hold);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the wait did not end");
}

#[tokio::test]
async fn all_synced_waits_for_the_end_of_each_list() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let mut started = avr.start().await.unwrap();
    let waiting = tokio::spawn(async move {
        started.all_synced().await;
        started
    });
    // Every query is answered except the sound mode list.
    let mut answered = 0;
    wait_until("the queries of the start", || {
        answer_new(&telnet, &mut answered, &["OPSMLALL ?"]);
        lines(&telnet.received()).contains(&"SSINFSIGECI ?")
    })
    .await;
    // A line of the list, and a power line from another client, do not end
    // the wait.
    telnet.send(b"OPSMLALL MOV011Stereo\rPWON\r");
    tokio::time::sleep(Duration::from_millis(500)).await;
    answer_new(&telnet, &mut answered, &["OPSMLALL ?"]);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(!waiting.is_finished());
    telnet.send(b"MV12\rOPSMLALL END\r");
    let mut started = timeout(Duration::from_secs(5), waiting)
        .await
        .expect("all_synced did not end")
        .unwrap();
    let report = started.all_synced().await;
    assert_eq!(report.telnet_sync, TelnetSync::Done);
    assert_eq!(report.telnet_not_answered, NONE);
    assert!(matches!(report.telnet, TelnetStart::Open));
    let volume = avr.snapshot().zone(Zone::Main).volume;
    assert_eq!(volume.map(denon_marantz::Volume::db), Some(-68.0));
}

#[tokio::test]
async fn all_synced_reports_the_queries_without_an_answer() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started_with(
        &web,
        TelnetConfig {
            answer_quiet: Duration::from_millis(300),
            ..telnet_config(telnet.port)
        },
    )
    .await;
    let mut started = avr.start().await.unwrap();
    let waiting = tokio::spawn(async move {
        started.all_synced().await;
        started
    });
    answer_until_done(&telnet, &waiting, &["PSIMAXSWM ?"]).await;
    let mut started = waiting.await.unwrap();
    let report = started.all_synced().await;
    assert_eq!(report.telnet_sync, TelnetSync::Done);
    assert_eq!(report.telnet_not_answered, ["PSIMAXSWM ?"]);
}

#[tokio::test]
async fn all_synced_reports_a_query_missed_before_it_was_called() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started_with(
        &web,
        TelnetConfig {
            answer_quiet: Duration::from_millis(300),
            ..telnet_config(telnet.port)
        },
    )
    .await;
    let mut started = avr.start().await.unwrap();
    let mut answered = 0;
    wait_until("the queries of the start", || {
        answer_new(&telnet, &mut answered, &["PSIMAXSWM ?"]);
        lines(&telnet.received()).contains(&"SSINFSIGECI ?")
    })
    .await;
    answer_new(&telnet, &mut answered, &["PSIMAXSWM ?"]);
    // Longer than the quiet time, so the query counts as not answered first.
    tokio::time::sleep(Duration::from_millis(1000)).await;
    let waiting = tokio::spawn(async move {
        started.all_synced().await;
        started
    });
    answer_until_done(&telnet, &waiting, &["PSIMAXSWM ?"]).await;
    let mut started = waiting.await.unwrap();
    let report = started.all_synced().await;
    assert_eq!(report.telnet_sync, TelnetSync::Done);
    assert_eq!(report.telnet_not_answered, ["PSIMAXSWM ?"]);
}

#[tokio::test]
async fn a_telnet_sync_that_is_dropped_does_not_stop_the_next_one() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    avr.start().await.unwrap().basic_synced().await;
    let _ = timeout(Duration::from_millis(1), avr.telnet_sync()).await;
    let second = {
        let avr = avr.clone();
        tokio::spawn(async move { avr.telnet_sync().await })
    };
    answer_until_done(&telnet, &second, &[]).await;
    assert_eq!(second.await.unwrap().unwrap(), NONE);
}

#[tokio::test]
async fn telnet_syncs_at_the_same_time_end_together() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    avr.start().await.unwrap().basic_synced().await;
    let (first, second) = (avr.clone(), avr.clone());
    let first = tokio::spawn(async move { first.telnet_sync().await });
    let second = tokio::spawn(async move { second.telnet_sync().await });
    answer_until_done(&telnet, &first, &[]).await;
    let second = timeout(Duration::from_secs(1), second)
        .await
        .expect("the second did not end");
    assert_eq!(first.await.unwrap().unwrap(), NONE);
    assert_eq!(second.unwrap().unwrap(), NONE);
}

#[tokio::test]
async fn all_synced_reports_a_connection_that_ends_before_the_answers() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let mut started = avr.start().await.unwrap();
    let waiting = tokio::spawn(async move {
        started.all_synced().await;
        started
    });
    wait_until("the queries of the start", || {
        lines(&telnet.received()).contains(&"SSINFSIGECI ?")
    })
    .await;
    telnet.close();
    let mut started = timeout(Duration::from_secs(5), waiting)
        .await
        .expect("all_synced did not end")
        .unwrap();
    let report = started.all_synced().await;
    assert_eq!(report.telnet_sync, TelnetSync::Lost);
}

#[tokio::test]
async fn all_synced_ends_with_no_answer_after_the_wait() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let mut started = avr
        .start_with(StartOptions::new().telnet_wait(Duration::from_millis(300)))
        .await
        .unwrap();
    // The fake telnet port never answers, and the quiet time is longer.
    let report = timeout(Duration::from_secs(5), started.all_synced())
        .await
        .expect("all_synced did not end");
    assert_eq!(report.telnet_sync, TelnetSync::NoAnswer);
    assert!(matches!(report.telnet, TelnetStart::Open));
    // The written queries are open, each once, as sent. The last query is
    // open too, written or not: on Windows, each write waits for a timer tick.
    let open = &report.telnet_not_answered;
    assert!(
        open.iter().any(|query| query == "SSINFSIGECI ?"),
        "{open:?}"
    );
    let received = telnet.received();
    let written = lines(&received);
    for query in written.iter().filter(|line| line.ends_with('?')) {
        assert_eq!(
            open.iter().filter(|open| open == query).count(),
            1,
            "{query}"
        );
    }
    // They can be asked again.
    let again: Vec<_> = open.iter().map(|query| Ask::Raw(query)).collect();
    avr.telnet_ask(&again).await.unwrap();
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
    let device: Vec<_> = fake_group_queries(QueryGroup::Device).collect();
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
    fake_group_queries(group)
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
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
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
    let last = fake_group_queries(QueryGroup::Device).last().unwrap();
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

    let mut started = avr.start().await.unwrap();

    let report = started.basic_synced().await;
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
    let mut started = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
    let report = started.all_synced().await;
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
    let zones = fake_group_queries(QueryGroup::Zones).count();
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

    let mut started = started;

    let report = started.basic_synced().await;
    assert!(matches!(report.telnet, TelnetStart::Open));
    let pages = report.web_interface.as_ref().unwrap();
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
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
    let pages = report.web_interface.as_ref().unwrap();
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
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
    assert_eq!(
        report.web_interface.as_ref().unwrap().video_signal,
        PageStatus::Read
    );
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
    let mut started = avr
        .start_with(StartOptions::new().signal_pages(false))
        .await
        .unwrap();
    let report = started.basic_synced().await;
    let pages = report.web_interface.as_ref().unwrap();
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
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
    assert!(
        matches!(report.telnet, TelnetStart::Failed(_)),
        "{:?}",
        report.telnet
    );
    // The pages were read anyway.
    assert!(
        report
            .web_interface
            .as_ref()
            .unwrap()
            .video_signal
            .is_read()
    );
}

#[tokio::test]
async fn a_start_that_is_dropped_leaves_telnet_closed() {
    let hang = Arc::new(AtomicBool::new(false));
    let http = avr_x_receiver_that_can_hang(hang.clone()).await;
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
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
    assert!(matches!(report.telnet, TelnetStart::Open));
    wait_until("the first query", || {
        lines(&telnet.received()).first() == Some(&"MSQUICK ?")
    })
    .await;
}

#[tokio::test]
async fn a_second_start_is_refused_while_the_first_runs() {
    let hang = Arc::new(AtomicBool::new(false));
    let http = avr_x_receiver_that_can_hang(hang.clone()).await;
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

/// The settings requests: the fast one asks for `GetBTTX`, the slow one for
/// `GetRestorerMode`.
fn settings_requests(http: &Fake) -> Vec<String> {
    appcommand0300_requests(http)
        .into_iter()
        .filter(|body| body.contains("GetBTTX") || body.contains("GetRestorerMode"))
        .collect()
}

#[tokio::test]
async fn a_start_without_telnet_reads_the_settings_over_http() {
    let web = web_interface().await;
    let http = avr_x_receiver_with_info(AVR_X_SETTINGS_INFO).await;
    let avr = connect_avr_x(&http, &web).await;
    let mut started = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
    started.basic_synced().await;
    // The fast settings are read before the report.
    let settings = avr.snapshot().device().sound_settings.clone();
    assert_eq!(settings.cinema_eq, Some(true));
    assert_eq!(settings.drc, Some(denon_marantz::proto::Drc::Off));
    assert_eq!(
        avr.snapshot().device().bluetooth_output,
        Some(denon_marantz::proto::BluetoothOutputMode::BluetoothAndSpeakers)
    );
    assert_eq!(
        avr.snapshot()
            .zone(Zone::Zone2)
            .bass
            .map(denon_marantz::ToneLevel::db),
        Some(-2)
    );
    // `all_synced` waits for the slow settings.
    started.all_synced().await;
    let state = avr.snapshot();
    assert_eq!(state.device().auto_lip_sync, Some(true));
    assert_eq!(state.device().sound_settings.audio_delay, Some(139));
    let requests = settings_requests(&http);
    assert_eq!(requests.len(), 2, "{requests:?}");
    assert!(requests[0].contains("z2bass"));
    // The receiver has two zones.
    assert!(!requests[0].contains("z3bass"));
}

#[tokio::test]
async fn a_start_with_telnet_does_not_read_the_settings_over_http() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let http = avr_x_receiver_with_info(AVR_X_SETTINGS_INFO).await;
    let avr = connected_with(&http, ReceiverType::AvrX, &web, telnet_config(telnet.port)).await;
    let mut started = avr
        .start_with(StartOptions::new().telnet_wait(Duration::from_millis(300)))
        .await
        .unwrap();
    assert!(matches!(
        started.all_synced().await.telnet,
        TelnetStart::Open
    ));
    assert_eq!(settings_requests(&http), Vec::<String>::new());
}

/// Telnet opens while the slow settings are on their way. Telnet asks for
/// its groups itself, so their values are skipped. Auto lip sync is in a
/// group that telnet asks for, the audio delay is not.
#[tokio::test]
async fn settings_of_the_groups_of_telnet_are_skipped_when_it_opens_meanwhile() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let release = Arc::new(AtomicBool::new(false));
    let http = avr_x_receiver_with_held_settings(Arc::clone(&release)).await;
    let avr = connected_with(&http, ReceiverType::AvrX, &web, telnet_config(telnet.port)).await;
    let mut started = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
    // `basic_synced` must not wait for the held answer.
    timeout(Duration::from_secs(5), started.basic_synced())
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    release.store(true, Ordering::SeqCst);
    timeout(Duration::from_secs(5), started.all_synced())
        .await
        .unwrap();
    let state = avr.snapshot();
    assert_eq!(state.device().auto_lip_sync, None);
    assert_eq!(state.device().sound_settings.audio_delay, Some(139));
}

/// Starts without telnet and waits for the end of the start.
async fn start_without_telnet(avr: &Arc<Avr>) {
    let mut started = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
    timeout(Duration::from_secs(10), started.all_synced())
        .await
        .unwrap();
}

/// Connects to the fake receiver, with the web interface.
async fn connect_avr_x(http: &Fake, web: &Fake) -> Arc<Avr> {
    Arc::new(
        builder(http, ReceiverType::AvrX)
            .web_interface_port(web.port)
            .web_interface_config(config())
            .connect()
            .await
            .unwrap(),
    )
}

#[tokio::test]
async fn a_receiver_that_has_no_query_list_is_not_asked_for_the_settings() {
    let web = web_interface().await;
    let http = avr_x_receiver().await;
    let avr = connect_avr_x(&http, &web).await;
    start_without_telnet(&avr).await;
    assert_eq!(settings_requests(&http), Vec::<String>::new());
}

#[tokio::test]
async fn a_receiver_that_lists_only_signal_queries_is_not_asked_for_the_settings() {
    let web = web_interface().await;
    let http = avr_x_receiver_with_info(AVR_X_OUTPUT_SETTINGS_INFO).await;
    let avr = connect_avr_x(&http, &web).await;
    start_without_telnet(&avr).await;
    assert!(avr.info().queries.is_some());
    let asked: Vec<_> = appcommand0300_requests(&http)
        .into_iter()
        .filter(|body| !body.contains("GetVideoInfo") && !body.contains("GetAudioInfo"))
        .collect();
    assert_eq!(asked, Vec::<String>::new());
}

#[tokio::test]
async fn an_older_receiver_is_not_asked_for_the_settings() {
    let web = web_interface().await;
    let http = avr_x_receiver_with_info(OLDER_SETTINGS_INFO).await;
    let avr = connect_avr_x(&http, &web).await;
    assert!(avr.info().queries.is_some_and(|queries| queries.zone_tone));
    start_without_telnet(&avr).await;
    avr.refresh().await.unwrap();
    assert_eq!(settings_requests(&http), Vec::<String>::new());
}

#[tokio::test]
async fn refresh_without_telnet_passes_when_the_settings_are_refused() {
    let web = web_interface().await;
    let http = avr_x_receiver_with_settings(AVR_X_SETTINGS_INFO, SettingsReplies::FastFails).await;
    let avr = connect_avr_x(&http, &web).await;
    avr.refresh().await.unwrap();
    assert_eq!(settings_requests(&http).len(), 1);
}

#[tokio::test]
async fn refresh_without_telnet_fails_when_the_settings_get_no_answer() {
    let web = web_interface().await;
    let http = avr_x_receiver_with_settings(AVR_X_SETTINGS_INFO, SettingsReplies::FastHangs).await;
    let avr = connect_avr_x(&http, &web).await;
    let error = timeout(Duration::from_secs(5), avr.refresh())
        .await
        .unwrap()
        .unwrap_err();
    assert!(error.is_unreachable(), "{error}");
    assert_eq!(settings_requests(&http).len(), 1);
}

#[tokio::test]
async fn a_start_with_the_settings_turned_off_does_not_read_them() {
    let web = web_interface().await;
    let http = avr_x_receiver_with_info(AVR_X_SETTINGS_INFO).await;
    let avr = connect_avr_x(&http, &web).await;
    let mut started = avr
        .start_with(StartOptions::new().telnet(false).settings(false))
        .await
        .unwrap();
    timeout(Duration::from_secs(10), started.all_synced())
        .await
        .unwrap();
    assert_eq!(settings_requests(&http), Vec::<String>::new());
}

#[tokio::test]
async fn a_fast_settings_read_without_answer_skips_the_slow_one() {
    let web = web_interface().await;
    let http = avr_x_receiver_with_settings(AVR_X_SETTINGS_INFO, SettingsReplies::FastHangs).await;
    let avr = connect_avr_x(&http, &web).await;
    let mut started = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
    let report = timeout(Duration::from_secs(10), started.all_synced())
        .await
        .unwrap();
    assert!(report.web_interface.is_ok());
    let requests = settings_requests(&http);
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert!(requests[0].contains("GetBTTX"));
}

#[tokio::test]
async fn all_synced_can_wait_again_for_the_slow_settings_after_a_timeout() {
    let SlowReadWaits {
        release,
        avr,
        mut started,
        ..
    } = slow_read_waits().await;
    // The timeout must stop the wait for the slow settings, not the start.
    assert!(
        timeout(Duration::from_millis(100), started.all_synced())
            .await
            .is_err()
    );
    assert_eq!(avr.snapshot().device().sound_settings.audio_delay, None);
    release.store(true, Ordering::SeqCst);
    timeout(Duration::from_secs(5), started.all_synced())
        .await
        .unwrap();
    assert_eq!(
        avr.snapshot().device().sound_settings.audio_delay,
        Some(139)
    );
}

/// Waits at most 100 ms until `avr` has no other owner.
async fn assert_only_owner(avr: &Arc<Avr>) {
    timeout(Duration::from_millis(100), async {
        while Arc::strong_count(avr) > 1 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("the start still holds the receiver");
}

#[tokio::test]
async fn a_stopped_start_ends_at_once_while_a_page_hangs() {
    let web = Fake::start(|_| Reply::Hang).await;
    let telnet = FakeTelnet::start().await;
    let avr = started(&web, &telnet).await;
    let mut started = avr.start().await.unwrap();
    wait_until("the video page request", || !web.requests().is_empty()).await;
    // The stopper works while another task waits.
    let stopper = started.stopper();
    let wait = tokio::spawn(async move {
        started.basic_synced().await;
        started
    });
    tokio::task::yield_now().await;
    stopper.stop();
    let mut started = timeout(Duration::from_millis(100), wait)
        .await
        .unwrap()
        .unwrap();
    let report = started.basic_synced().await;
    assert!(
        matches!(report.web_interface, Err(Error::Stopped)),
        "{:?}",
        report.web_interface
    );
    drop(started);
    assert_only_owner(&avr).await;
    // The stop ended the start, so another one runs.
    avr.start().await.unwrap();
}

/// A start without telnet whose slow settings request waits for `release`.
struct SlowReadWaits {
    _web: Fake,
    _http: Fake,
    release: Arc<AtomicBool>,
    avr: Arc<Avr>,
    started: Started,
}

/// Starts it and returns after the fake got that request. The task has sent
/// the report then, but it is not read.
async fn slow_read_waits() -> SlowReadWaits {
    let web = web_interface().await;
    let release = Arc::new(AtomicBool::new(false));
    let http = avr_x_receiver_with_held_settings(Arc::clone(&release)).await;
    let avr = connect_avr_x(&http, &web).await;
    let started = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
    wait_until("the slow settings request", || {
        settings_requests(&http)
            .iter()
            .any(|body| body.contains("GetRestorerMode"))
    })
    .await;
    SlowReadWaits {
        _web: web,
        _http: http,
        release,
        avr,
        started,
    }
}

#[tokio::test]
async fn stop_ends_the_slow_settings_read_and_keeps_the_report() {
    let SlowReadWaits {
        release,
        avr,
        mut started,
        ..
    } = slow_read_waits().await;
    started.basic_synced().await;
    started.stop();
    let report = timeout(Duration::from_millis(100), started.all_synced())
        .await
        .unwrap();
    assert!(report.web_interface.is_ok(), "{:?}", report.web_interface);
    drop(started);
    assert_only_owner(&avr).await;
    release.store(true, Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(avr.snapshot().device().sound_settings.audio_delay, None);
}

#[tokio::test]
async fn a_report_sent_before_the_stop_and_read_after_it_is_kept() {
    let SlowReadWaits { mut started, .. } = slow_read_waits().await;
    started.stop();
    let report = timeout(Duration::from_millis(100), started.basic_synced())
        .await
        .unwrap();
    assert!(report.web_interface.is_ok(), "{:?}", report.web_interface);
}

#[tokio::test]
async fn a_stop_does_not_end_the_telnet_wait_of_all_synced() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let mut started = avr
        .start_with(StartOptions::new().telnet_wait(Duration::from_millis(300)))
        .await
        .unwrap();
    started.basic_synced().await;
    // With telnet the slow settings are not read, so the task has ended. The
    // telnet wait runs in the task that calls `all_synced`.
    let stopper = started.stopper();
    let wait = tokio::spawn(async move {
        started.all_synced().await;
        started
    });
    tokio::task::yield_now().await;
    stopper.stop();
    let mut started = timeout(Duration::from_secs(2), wait)
        .await
        .unwrap()
        .unwrap();
    let report = started.all_synced().await;
    assert_eq!(report.telnet_sync, TelnetSync::NoAnswer);
    assert!(report.web_interface.is_ok(), "{:?}", report.web_interface);
}

#[tokio::test]
async fn a_stopper_works_after_started_is_dropped() {
    let SlowReadWaits { avr, started, .. } = slow_read_waits().await;
    let stopper = started.stopper();
    drop(started);
    stopper.stop();
    assert_only_owner(&avr).await;
}

#[tokio::test]
async fn refresh_with_telnet_does_not_read_the_settings() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let http = avr_x_receiver_with_info(AVR_X_SETTINGS_INFO).await;
    let avr = connected_with(&http, ReceiverType::AvrX, &web, telnet_config(telnet.port)).await;
    avr.telnet_connect().await.unwrap();
    let _ = timeout(Duration::from_secs(5), avr.refresh())
        .await
        .unwrap();
    assert_eq!(settings_requests(&http), Vec::<String>::new());
}

#[tokio::test]
async fn refresh_without_telnet_reads_the_settings_again() {
    let web = web_interface().await;
    let http = avr_x_receiver_with_info(AVR_X_SETTINGS_INFO).await;
    let avr = connect_avr_x(&http, &web).await;
    avr.refresh().await.unwrap();
    assert_eq!(settings_requests(&http).len(), 2);
    assert_eq!(
        avr.snapshot().device().sound_settings.audio_delay,
        Some(139)
    );
}

#[tokio::test]
async fn a_failed_fast_settings_read_skips_the_slow_one() {
    let web = web_interface().await;
    let http = avr_x_receiver_with_settings(AVR_X_SETTINGS_INFO, SettingsReplies::FastFails).await;
    let avr = connect_avr_x(&http, &web).await;
    start_without_telnet(&avr).await;
    let requests = settings_requests(&http);
    assert_eq!(requests.len(), 1, "{requests:?}");
    assert!(requests[0].contains("GetBTTX"));
}

/// The settings are on the same port as the signal, so they would only wait
/// for the timeout too.
#[tokio::test]
async fn a_signal_read_without_answer_skips_the_settings() {
    let web = web_interface().await;
    let http =
        avr_x_receiver_with_settings(AVR_X_SETTINGS_AND_SIGNAL_INFO, SettingsReplies::SignalHangs)
            .await;
    let avr = connect_avr_x(&http, &web).await;
    let mut started = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
    let report = timeout(Duration::from_secs(10), started.all_synced())
        .await
        .unwrap();
    assert_eq!(report.signal, PageStatus::NoAnswer);
    assert_eq!(settings_requests(&http), Vec::<String>::new());
}

#[tokio::test]
async fn a_start_that_leaves_open_telnet_alone_does_not_read_the_settings() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let http = avr_x_receiver_with_info(AVR_X_SETTINGS_INFO).await;
    let avr = connected_with(&http, ReceiverType::AvrX, &web, telnet_config(telnet.port)).await;
    avr.telnet_connect().await.unwrap();
    start_without_telnet(&avr).await;
    assert_eq!(settings_requests(&http), Vec::<String>::new());
}

#[tokio::test]
async fn a_receiver_from_stored_values_reads_the_settings() {
    let web = web_interface().await;
    let http = avr_x_receiver_with_info(AVR_X_SETTINGS_INFO).await;
    let info = connect_avr_x(&http, &web).await.info().clone();
    let avr = Arc::new(
        Avr::builder("127.0.0.1")
            .http_config(config())
            .web_interface_port(web.port)
            .web_interface_config(config())
            .known(info)
            .connect()
            .await
            .unwrap(),
    );
    start_without_telnet(&avr).await;
    assert_eq!(settings_requests(&http).len(), 2);
    assert_eq!(
        avr.snapshot().device().sound_settings.audio_delay,
        Some(139)
    );
}

#[tokio::test]
async fn stored_values_without_the_settings_queries_read_no_settings_and_are_changed() {
    // Values stored by an older version have no version 2 of
    // `GetSurroundParameter`.
    let web = web_interface().await;
    let http = avr_x_receiver_with_info(AVR_X_SETTINGS_INFO).await;
    let info = connect_avr_x(&http, &web).await.info().clone();
    let mut old = info.clone();
    old.queries.as_mut().unwrap().surround_parameter = false;
    let avr = Arc::new(
        Avr::builder("127.0.0.1")
            .http_config(config())
            .web_interface_port(web.port)
            .web_interface_config(config())
            .known(old)
            .connect()
            .await
            .unwrap(),
    );
    let before = settings_requests(&http).len();
    let mut started = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
    match started.info_check().await {
        InfoCheck::Changed(new) => assert_eq!(**new, info),
        other => panic!("{other:?}"),
    }
    timeout(Duration::from_secs(10), started.all_synced())
        .await
        .unwrap();
    assert_eq!(settings_requests(&http).len(), before);
}

#[tokio::test]
async fn a_start_with_telnet_running_does_not_read_the_settings_over_http() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let http = avr_x_receiver_with_info(AVR_X_SETTINGS_INFO).await;
    let avr = connected_with(&http, ReceiverType::AvrX, &web, telnet_config(telnet.port)).await;
    avr.telnet_connect().await.unwrap();
    let mut started = avr
        .start_with(StartOptions::new().telnet_wait(Duration::from_millis(300)))
        .await
        .unwrap();
    assert!(matches!(
        started.all_synced().await.telnet,
        TelnetStart::AlreadyRunning
    ));
    assert_eq!(settings_requests(&http), Vec::<String>::new());
}

#[tokio::test]
async fn a_start_with_a_closed_telnet_port_reads_the_settings_over_http() {
    let web = web_interface().await;
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = closed.local_addr().unwrap().port();
    drop(closed);
    let http = avr_x_receiver_with_info(AVR_X_SETTINGS_INFO).await;
    let avr = connected_with(&http, ReceiverType::AvrX, &web, telnet_config(port)).await;
    let mut started = avr.start().await.unwrap();
    assert!(matches!(
        started.all_synced().await.telnet,
        TelnetStart::Failed(_)
    ));
    assert_eq!(settings_requests(&http).len(), 2);
}

#[tokio::test]
async fn a_start_while_telnet_connects_again_reads_the_settings_over_http() {
    let web = web_interface().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let http = avr_x_receiver_with_info(AVR_X_SETTINGS_INFO).await;
    let avr = connected_with(&http, ReceiverType::AvrX, &web, telnet_config(port)).await;
    // One connection, then the port is closed, so the task keeps trying.
    let accept = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        drop(listener);
        drop(stream);
    });
    avr.telnet_connect().await.unwrap();
    accept.await.unwrap();
    let mut status = avr.telnet_status();
    timeout(
        Duration::from_secs(2),
        status.wait_for(|status| *status != TelnetStatus::Connected),
    )
    .await
    .unwrap()
    .unwrap();
    let mut started = avr.start().await.unwrap();
    assert!(matches!(
        started.basic_synced().await.telnet,
        TelnetStart::AlreadyRunning
    ));
    wait_until("the slow settings", || settings_requests(&http).len() == 2).await;
}

#[tokio::test]
async fn telnet_can_be_left_closed() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let mut started = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
    let report = started.basic_synced().await;
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
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
    assert!(report.web_interface.as_ref().unwrap_err().is_unreachable());
    // Telnet gives the signal then.
    wait_until("the signal queries", || {
        let received = telnet.received();
        let sent = lines(&received);
        sent.contains(&"SSINFSIGRES ?") && sent.contains(&"OPINFASP ?")
    })
    .await;
}

#[tokio::test]
async fn refresh_signal_with_telnet_asks_only_telnet() {
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
    let pages = web.requests().len();
    let (eci, audio) = (count("SSINFSIGECI ?"), count("SYSDA ?"));
    avr.refresh_signal().await.unwrap();
    wait_until("the signal queries", || {
        count("SSINFSIGECI ?") > eci && count("SYSDA ?") > audio
    })
    .await;
    let received = telnet.received();
    let sent = lines(&received[before..]);
    for query in ["SYSDA ?", "SSINFSIGRES ?", "OPINFASP ?", "SSINFSIGECI ?"] {
        assert!(sent.contains(&query), "{query} {sent:?}");
    }
    // BT.2020 last, so that its answer comes after the video values.
    assert_eq!(sent.last(), Some(&"SSINFSIGECI ?"));
    assert_eq!(web.requests().len(), pages, "no HTTP while telnet is open");
}

/// An `Avr` with open telnet that asked only `ZM?` after the connect.
async fn quiet_telnet() -> (Fake, FakeTelnet, Arc<Avr>) {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let http = avr_x_receiver().await;
    let avr = connected(&http, ReceiverType::AvrX, &web, &telnet, &[]).await;
    avr.telnet_connect().await.unwrap();
    wait_until("the query of the connect", || {
        lines(&telnet.received()).contains(&"ZM?")
    })
    .await;
    (web, telnet, avr)
}

#[tokio::test]
async fn refresh_signal_pages_with_telnet_reads_only_the_pages() {
    let (web, telnet, avr) = quiet_telnet().await;
    let before = telnet.received().len();
    let read = |path: &str| {
        web.requests()
            .iter()
            .filter(|seen| seen.path == path)
            .count()
    };
    avr.refresh_signal_pages().await.unwrap();
    assert_eq!(read(VIDEO_SIGNAL_PATH), 1);
    assert_eq!(read(AUDIO_CHANNELS_PATH), 1);
    assert!(avr.snapshot().device().signal.channels.input.is_some());
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(telnet.received().len(), before, "no telnet queries");
}

/// How often [`Avr::refresh_signal_pages`] reads the channels page, for a
/// receiver with this device information and this `AppCommand0300.xml` answer.
async fn channels_page_reads(info: &'static [u8], answer: String) -> usize {
    let web = web_interface().await;
    // `Avr::update` is not called, so tone control does not matter.
    let receiver = common::receiver_with_info_and_signal_answers(info, false, move |_| {
        Reply::Body(answer.clone().into_bytes())
    })
    .await;
    let avr = builder(&receiver, ReceiverType::AvrX2016)
        .web_interface_port(web.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    avr.refresh_signal_pages().await.unwrap();
    assert!(avr.snapshot().device().signal.channels.input.is_some());
    web.requests()
        .iter()
        .filter(|seen| seen.path == AUDIO_CHANNELS_PATH)
        .count()
}

#[tokio::test]
async fn refresh_signal_pages_reads_the_channels_page_only_without_channels_from_appcommand0300() {
    let full = common::appcommand0300_answer("");
    assert_eq!(
        channels_page_reads(AVR_X_OUTPUT_SETTINGS_INFO, full.clone()).await,
        0
    );
    // The channel queries are not asked.
    assert_eq!(
        channels_page_reads(common::AUDIO_INFO_ONLY_INFO, full.clone()).await,
        1
    );
    // The answer has no item for the speakers.
    let without_speakers = full.replace(&common::appcommand0300_item("GetActiveSpeaker"), "");
    assert_eq!(
        channels_page_reads(AVR_X_OUTPUT_SETTINGS_INFO, without_speakers).await,
        1
    );
    // With an item for each asked command, the order of the items decides
    // which command an error item is for.
    let all = full.replace(
        "</rx>",
        &format!("{}</rx>", common::output_settings_item("2")),
    );
    let error_for =
        |name: &str| all.replace(&common::appcommand0300_item(name), "<error>2</error>");
    // An error item for another command does not matter.
    assert_eq!(
        channels_page_reads(AVR_X_OUTPUT_SETTINGS_INFO, error_for("GetVideoInfo")).await,
        0
    );
    assert_eq!(
        channels_page_reads(AVR_X_OUTPUT_SETTINGS_INFO, error_for("GetActiveSpeaker")).await,
        1
    );
}

#[tokio::test]
async fn refresh_signal_pages_keeps_the_channels_when_an_answer_leaves_them_out() {
    let web = web_interface().await;
    let full = common::appcommand0300_answer("");
    let without_speakers = full.replace(&common::appcommand0300_item("GetActiveSpeaker"), "");
    // 0: full answer, 1: without the speakers, 2: refused.
    let mode = Arc::new(AtomicUsize::new(0));
    let answer_mode = Arc::clone(&mode);
    // The start calls `Avr::update`, which asks tone control.
    let receiver = common::receiver_with_info_and_signal_answers(
        AVR_X_OUTPUT_SETTINGS_INFO,
        true,
        move |_| match answer_mode.load(Ordering::Relaxed) {
            0 => Reply::Body(full.clone().into_bytes()),
            1 => Reply::Body(without_speakers.clone().into_bytes()),
            _ => Reply::Status(500),
        },
    )
    .await;
    let avr = Arc::new(
        builder(&receiver, ReceiverType::AvrX2016)
            .web_interface_port(web.port)
            .web_interface_config(config())
            .connect()
            .await
            .unwrap(),
    );
    let mut started = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
    started.basic_synced().await;
    let channels = avr.snapshot().device().signal.channels.clone();
    assert!(channels.input.is_some());
    let asked = || {
        web.requests()
            .iter()
            .filter(|seen| seen.path == AUDIO_CHANNELS_PATH)
            .count()
    };
    mode.store(1, Ordering::Relaxed);
    avr.refresh_signal_pages().await.unwrap();
    assert_eq!(asked(), 0);
    assert_eq!(avr.snapshot().device().signal.channels, channels);
    // Without an answer of `AppCommand0300.xml`, the page is read.
    mode.store(2, Ordering::Relaxed);
    avr.refresh_signal_pages().await.unwrap();
    assert_eq!(asked(), 1);
}

/// An `Avr` for a receiver with this device information and the full
/// `AppCommand0300.xml` answer, with a web interface that answers 404 for all
/// pages.
async fn avr_without_signal_pages(info: &'static [u8]) -> (Fake, Avr) {
    let web = Fake::start(|_| Reply::Status(404)).await;
    let answer = common::appcommand0300_answer("");
    // `Avr::update` is not called, so tone control does not matter.
    let receiver = common::receiver_with_info_and_signal_answers(info, false, move |_| {
        Reply::Body(answer.clone().into_bytes())
    })
    .await;
    let avr = builder(&receiver, ReceiverType::AvrX2016)
        .web_interface_port(web.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    (web, avr)
}

#[tokio::test]
async fn a_video_page_that_answers_404_is_not_asked_again_and_leaves_http_reads() {
    let (web, avr) = avr_without_signal_pages(AVR_X_OUTPUT_SETTINGS_INFO).await;
    let both = HttpReads::SIGNAL_AND_SETTINGS | HttpReads::SIGNAL_PAGES;
    assert_eq!(avr.http_reads(Query::HdrFormat), HttpReads::SIGNAL_PAGES);
    avr.refresh_signal_pages().await.unwrap();
    avr.refresh_signal_pages().await.unwrap();
    let asked = |path: &str| {
        web.requests()
            .iter()
            .filter(|seen| seen.path == path)
            .count()
    };
    assert_eq!(asked(VIDEO_SIGNAL_PATH), 1);
    // `AppCommand0300.xml` gave the channels, so their page is not asked.
    assert_eq!(asked(AUDIO_CHANNELS_PATH), 0);
    assert_eq!(avr.http_reads(Query::HdrFormat), HttpReads::EMPTY);
    assert_eq!(
        avr.info().http_reads(Query::HdrFormat),
        HttpReads::SIGNAL_PAGES
    );
    // The values of `AppCommand0300.xml` stay.
    assert_eq!(avr.http_reads(Query::VideoResolution), both);
    assert_eq!(avr.http_reads(Query::InputChannels), both);
}

#[tokio::test]
async fn a_channels_page_that_answers_404_is_not_asked_again_and_leaves_http_reads() {
    let (web, avr) = avr_without_signal_pages(common::AUDIO_INFO_ONLY_INFO).await;
    assert_eq!(
        avr.http_reads(Query::InputChannels),
        HttpReads::SIGNAL_PAGES
    );
    avr.refresh_signal_pages().await.unwrap();
    avr.refresh_signal_pages().await.unwrap();
    let channels = web
        .requests()
        .iter()
        .filter(|seen| seen.path == AUDIO_CHANNELS_PATH)
        .count();
    assert_eq!(channels, 1);
    assert_eq!(avr.http_reads(Query::InputChannels), HttpReads::EMPTY);
    assert_eq!(
        avr.http_reads(Query::SamplingRate),
        HttpReads::SIGNAL_AND_SETTINGS | HttpReads::SIGNAL_PAGES
    );
}

#[tokio::test]
async fn stored_missing_sources_are_not_asked_again_with_known_values() {
    let (web, avr) = avr_without_signal_pages(common::AUDIO_INFO_ONLY_INFO).await;
    // `Deviceinfo.xml` does not list tone control.
    let at_connect = MissingSources {
        tone_control: true,
        ..MissingSources::default()
    };
    assert_eq!(avr.missing_sources(), at_connect);
    avr.refresh_signal_pages().await.unwrap();
    let missing = avr.missing_sources();
    let expected = MissingSources {
        video_page: true,
        channels_page: true,
        ..at_connect
    };
    assert_eq!(missing, expected);
    for &query in Query::ALL {
        assert_eq!(
            avr.info().http_reads_without(query, missing),
            avr.http_reads(query),
            "{query:?}"
        );
    }
    let info = avr.info().clone();
    drop(avr);
    // The probes of `builder` are not used with known values.
    let known = |missing| {
        builder(&web, info.receiver_type)
            .web_interface_port(web.port)
            .web_interface_config(config())
            .known(info.clone())
            .missing(missing)
    };
    let before = web.requests().len();
    let avr = known(missing).connect().await.unwrap();
    assert_eq!(avr.missing_sources(), missing);
    assert_eq!(avr.http_reads(Query::InputChannels), HttpReads::EMPTY);
    avr.refresh_signal_pages().await.unwrap();
    assert_eq!(web.requests().len(), before);
    // Without stored sources, the pages are asked again.
    let avr = known(MissingSources::default()).connect().await.unwrap();
    avr.refresh_signal_pages().await.unwrap();
    assert_eq!(web.requests().len(), before + 2);
    // A connect without known values starts with no missing source.
    let avr = builder(&web, info.receiver_type)
        .probes(&[(info.receiver_type, info.port)])
        .description_port(info.port)
        .web_interface_port(web.port)
        .web_interface_config(config())
        .missing(missing)
        .connect()
        .await
        .unwrap();
    assert_eq!(avr.missing_sources(), at_connect);
}

#[tokio::test]
async fn refresh_signal_pages_asks_telnet_for_bt2020_after_a_change() {
    let (_web, telnet, avr) = quiet_telnet().await;
    telnet.send(b"SSINFSIGRES I1080p50\rSSINFSIGCOS 11\rSSINFSIGECI 55\r");
    wait_until("BT.2020", || {
        avr.snapshot().device().signal.bt2020.input == Some(true)
    })
    .await;
    assert!(!lines(&telnet.received()).contains(&"SSINFSIGECI ?"));
    // The page has another resolution, so BT.2020 is not known.
    avr.refresh_signal_pages().await.unwrap();
    wait_until("the colorimetry query", || {
        lines(&telnet.received()).contains(&"SSINFSIGECI ?")
    })
    .await;
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
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
    // The web interface answered, so the report has the pages that were read.
    let pages = report.web_interface.as_ref().unwrap();
    assert_eq!(pages.availability.pages.len(), 7);
    assert_eq!(pages.video_signal, PageStatus::NoAnswer);
    assert_eq!(pages.audio_channels, PageStatus::Read);
    // The request was sent, so the video page is not read again.
    let video_reads = web
        .requests()
        .iter()
        .filter(|seen| seen.path == VIDEO_SIGNAL_PATH)
        .count();
    assert_eq!(video_reads, 1);
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

/// The device information of a receiver that lists the channel queries of
/// `AppCommand0300.xml`, but not `GetAudioInfo`.
const CHANNELS_ONLY_INFO: &[u8] =
    b"<Device_Info><CommApiVers>0301</CommApiVers><ModelName>AVR-X3700H</ModelName>\
    <DeviceZones>2</DeviceZones><DeviceCapabilities><Setup>\
    <StatusInfo><Commands><GetInputSignal ver=\"1\">1</GetInputSignal>\
    <GetActiveSpeaker ver=\"1\">1</GetActiveSpeaker></Commands></StatusInfo>\
    </Setup></DeviceCapabilities></Device_Info>";

/// Starts a receiver with this device information and the full
/// `AppCommand0300.xml` answer. Gives the status of the channels page and the
/// telnet lines after the queue is done.
async fn start_signal_reads(info: &'static [u8]) -> (PageStatus, String) {
    let web = web_interface().await;
    let answer = common::appcommand0300_answer("");
    // Neither info lists tone control.
    let receiver = common::receiver_with_info_and_signal_answers(info, false, move |_| {
        Reply::Body(answer.clone().into_bytes())
    })
    .await;
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
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
    let channels = report.web_interface.as_ref().unwrap().audio_channels;
    wait_for_the_queue(&avr, &telnet).await;
    (channels, telnet.received())
}

#[tokio::test]
async fn the_start_reads_the_channels_page_when_appcommand0300_has_no_channel_queries() {
    let (channels, sent) = start_signal_reads(common::AUDIO_INFO_ONLY_INFO).await;
    assert_eq!(channels, PageStatus::Read);
    assert!(!sent.contains("SYSDA ?"), "{sent}");
}

#[tokio::test]
async fn the_start_asks_telnet_for_the_audio_signal_when_appcommand0300_has_no_audio_query() {
    let (channels, sent) = start_signal_reads(CHANNELS_ONLY_INFO).await;
    assert_eq!(channels, PageStatus::Skipped(SkipReason::AlreadyRead));
    assert!(sent.contains("SYSDA ?"), "{sent}");
    assert!(!sent.contains("OPINFINS ?"), "{sent}");
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
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
    let pages = report.web_interface.as_ref().unwrap();
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
async fn the_start_reads_the_video_page_first_and_appcommand0300_in_two_requests() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let web_order = Arc::clone(&order);
    let web = Fake::start(move |seen| {
        web_order.lock().unwrap().push(seen.path.clone());
        match seen.path.as_str() {
            VIDEO_SIGNAL_PATH => Reply::Body(VIDEO_PAGE.to_vec()),
            _ => Reply::Status(404),
        }
    })
    .await;
    let answer = common::appcommand0300_answer("");
    let http_order = Arc::clone(&order);
    let receiver = common::receiver_with_signal_answers(true, move |asked| {
        http_order
            .lock()
            .unwrap()
            .push(format!("AppCommand0300 {asked}"));
        Reply::Body(answer.clone().into_bytes())
    })
    .await;
    let telnet = FakeTelnet::start().await;
    let avr = connected(
        &receiver,
        ReceiverType::AvrX2016,
        &web,
        &telnet,
        QueryGroup::START_ORDER,
    )
    .await;
    avr.start().await.unwrap().basic_synced().await;
    let order = order.lock().unwrap().clone();
    assert_eq!(
        order[..3],
        [VIDEO_SIGNAL_PATH, "AppCommand0300 0", "AppCommand0300 1"]
    );
    // The audio signal and the channels are in the first request.
    let bodies: Vec<String> = receiver
        .requests()
        .iter()
        .filter(|seen| seen.path == "/goform/AppCommand0300.xml")
        .map(|seen| String::from_utf8_lossy(&seen.body).into_owned())
        .collect();
    assert_eq!(bodies.len(), 2);
    for name in ["GetAudioInfo", "GetInputSignal", "GetActiveSpeaker"] {
        assert!(
            bodies[0].contains(name) && !bodies[1].contains(name),
            "{name}"
        );
    }
    for name in ["GetAudyssey", "GetVideoInfo"] {
        assert!(
            bodies[1].contains(name) && !bodies[0].contains(name),
            "{name}"
        );
    }
    // Both answers are in the state, so the channels page is not read.
    let state = avr.snapshot();
    assert_eq!(
        state.device().signal.audio_signal.as_deref(),
        Some("Dolby Atmos")
    );
    assert_eq!(
        state.device().signal.channels.input.unwrap().to_string(),
        "2.0"
    );
    assert!(!order.iter().any(|path| path == AUDIO_CHANNELS_PATH));
}

#[tokio::test]
async fn a_web_interface_that_hangs_is_asked_for_two_pages_only() {
    let web = Fake::start(|_| Reply::Hang).await;
    let telnet = FakeTelnet::start().await;
    let avr = started(&web, &telnet).await;
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
    // Neither the video page nor the first page after it answered.
    assert!(report.web_interface.as_ref().unwrap_err().is_unreachable());
    assert_eq!(web.requests().len(), 2);
}

#[tokio::test]
async fn a_second_start_after_a_video_page_404_stops_at_a_web_interface_that_hangs() {
    let hang = Arc::new(AtomicBool::new(false));
    let web = {
        let hang = hang.clone();
        Fake::start(move |_| {
            if hang.load(Ordering::SeqCst) {
                Reply::Hang
            } else {
                Reply::Status(404)
            }
        })
        .await
    };
    let telnet = FakeTelnet::start().await;
    let avr = started(&web, &telnet).await;
    {
        let mut started = avr.start().await.unwrap();
        let report = started.basic_synced().await;
        let pages = report.web_interface.as_ref().unwrap();
        assert_eq!(pages.video_signal, PageStatus::NotFound);
    }
    hang.store(true, Ordering::SeqCst);
    let before = web.requests().len();
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
    // The video page is not asked again, so the first page after it decides.
    assert!(report.web_interface.as_ref().unwrap_err().is_unreachable());
    assert_eq!(web.requests().len() - before, 1);
}

#[tokio::test]
async fn refresh_signal_pages_tells_an_unreachable_receiver_after_a_video_page_404() {
    let web = Fake::start(|_| Reply::Status(404)).await;
    let hang = Arc::new(AtomicBool::new(false));
    let answer = common::appcommand0300_answer("");
    let receiver = {
        let hang = hang.clone();
        // `Avr::update` is not called, so tone control does not matter.
        common::receiver_with_info_and_signal_answers(
            AVR_X_OUTPUT_SETTINGS_INFO,
            false,
            move |_| {
                if hang.load(Ordering::SeqCst) {
                    Reply::Hang
                } else {
                    Reply::Body(answer.clone().into_bytes())
                }
            },
        )
        .await
    };
    let avr = builder(&receiver, ReceiverType::AvrX2016)
        .web_interface_port(web.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    avr.refresh_signal_pages().await.unwrap();
    hang.store(true, Ordering::SeqCst);
    let error = avr.refresh_signal_pages().await.unwrap_err();
    assert!(error.is_unreachable(), "{error:?}");
}

#[tokio::test]
async fn a_video_page_that_fails_with_500_is_asked_again() {
    let web = Fake::start(|seen| match seen.path.as_str() {
        VIDEO_SIGNAL_PATH => Reply::Status(500),
        _ => Reply::Status(404),
    })
    .await;
    let answer = common::appcommand0300_answer("");
    // `Avr::update` is not called, so tone control does not matter.
    let receiver = common::receiver_with_info_and_signal_answers(
        AVR_X_OUTPUT_SETTINGS_INFO,
        false,
        move |_| Reply::Body(answer.clone().into_bytes()),
    )
    .await;
    let avr = builder(&receiver, ReceiverType::AvrX2016)
        .web_interface_port(web.port)
        .web_interface_config(config())
        .connect()
        .await
        .unwrap();
    avr.refresh_signal_pages().await.unwrap();
    avr.refresh_signal_pages().await.unwrap();
    let video = web
        .requests()
        .iter()
        .filter(|seen| seen.path == VIDEO_SIGNAL_PATH)
        .count();
    // One more try after each 500.
    assert_eq!(video, 4);
    assert_eq!(avr.http_reads(Query::HdrFormat), HttpReads::SIGNAL_PAGES);
}

#[tokio::test]
async fn a_failed_second_appcommand0300_request_keeps_the_first_answer() {
    let answer = common::appcommand0300_answer("");
    let receiver = common::receiver_with_signal_answers(true, move |asked| {
        if asked == 0 {
            Reply::Body(answer.clone().into_bytes())
        } else {
            Reply::Status(404)
        }
    })
    .await;
    let web = web_interface().await;
    let telnet = FakeTelnet::start().await;
    let avr = connected(
        &receiver,
        ReceiverType::AvrX2016,
        &web,
        &telnet,
        QueryGroup::START_ORDER,
    )
    .await;
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
    assert_eq!(report.signal, PageStatus::Read);
    assert_eq!(
        avr.snapshot().device().signal.audio_signal.as_deref(),
        Some("Dolby Atmos")
    );
    // The first request gave the channels. The page has none for a stream
    // of objects, and would clear them.
    let pages = report.web_interface.as_ref().unwrap();
    assert_eq!(
        pages.audio_channels,
        PageStatus::Skipped(SkipReason::AlreadyRead)
    );
    // Telnet is not asked again for what the first request gave.
    wait_for_the_queue(&avr, &telnet).await;
    let text = telnet.received();
    let sent = lines(&text);
    for query in ["SYSDA ?", "SYSMI ?", "SSINFAISFSV ?", "OPINFINS ?"] {
        assert!(!sent.contains(&query), "{query}");
    }
}

#[tokio::test]
async fn a_second_appcommand0300_request_without_answer_keeps_the_first_answer() {
    let answer = common::appcommand0300_answer("");
    let receiver = common::receiver_with_signal_answers(true, move |asked| {
        if asked == 0 {
            Reply::Body(answer.clone().into_bytes())
        } else {
            Reply::Hang
        }
    })
    .await;
    let web = web_interface().await;
    let telnet = FakeTelnet::start().await;
    let avr = connected(
        &receiver,
        ReceiverType::AvrX2016,
        &web,
        &telnet,
        QueryGroup::START_ORDER,
    )
    .await;
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
    assert_eq!(report.signal, PageStatus::Read);
    assert_eq!(
        avr.snapshot().device().signal.audio_signal.as_deref(),
        Some("Dolby Atmos")
    );
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

#[tokio::test]
async fn the_start_checks_the_known_values() {
    let (http, web) = (avr_x_receiver().await, web_interface().await);
    let builder = || {
        Avr::builder("127.0.0.1")
            .http_config(config())
            .web_interface_port(web.port)
            .web_interface_config(config())
    };
    let options = StartOptions::new().telnet(false);
    let read = Arc::new(
        builder()
            .probes(&[(ReceiverType::AvrX, http.port)])
            .description_port(http.port)
            .connect()
            .await
            .unwrap(),
    );
    let info = read.info().clone();
    let mut started = read.start_with(options).await.unwrap();
    assert!(matches!(started.info_check().await, InfoCheck::NotNeeded));

    let same = Arc::new(builder().known(info.clone()).connect().await.unwrap());
    let mut started = same.start_with(options).await.unwrap();
    assert!(matches!(started.info_check().await, InfoCheck::Same));
    // It keeps the result.
    assert!(matches!(started.info_check().await, InfoCheck::Same));

    let mut old = info.clone();
    old.friendly_name = "Old name".to_owned();
    let changed = Arc::new(builder().known(old).connect().await.unwrap());
    let mut started = changed.start_with(options).await.unwrap();
    match started.info_check().await {
        InfoCheck::Changed(new) => assert_eq!(**new, info),
        other => panic!("{other:?}"),
    }
}

// The task must not run before the stop. This needs a runtime with one thread.
#[tokio::test(flavor = "current_thread")]
async fn a_stop_before_the_check_of_known_values_fails_the_check() {
    let (http, web) = (avr_x_receiver().await, web_interface().await);
    let info = builder(&http, ReceiverType::AvrX)
        .connect()
        .await
        .unwrap()
        .info()
        .clone();
    let avr = Arc::new(
        Avr::builder("127.0.0.1")
            .http_config(config())
            .web_interface_port(web.port)
            .web_interface_config(config())
            .known(info)
            .connect()
            .await
            .unwrap(),
    );
    let mut started = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
    // The task has not run yet on this runtime, so the check never starts.
    started.stop();
    assert!(matches!(
        started.info_check().await,
        InfoCheck::Failed(Error::Stopped)
    ));
    // The wait ends after the task: a new start runs.
    started.basic_synced().await;
    avr.start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn a_stop_during_the_check_of_known_values_fails_the_check() {
    let (hang, web) = (Arc::new(AtomicBool::new(false)), web_interface().await);
    let http = avr_x_receiver_that_can_hang(Arc::clone(&hang)).await;
    let info = builder(&http, ReceiverType::AvrX)
        .connect()
        .await
        .unwrap()
        .info()
        .clone();
    let avr = Arc::new(
        Avr::builder("127.0.0.1")
            .http_config(config())
            .web_interface_port(web.port)
            .web_interface_config(config())
            .known(info)
            .connect()
            .await
            .unwrap(),
    );
    let mut started = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
    // The task has not run yet on this runtime with one thread, so the
    // check sends its requests after this.
    hang.store(true, Ordering::SeqCst);
    let before = http.requests().len();
    wait_until("the AppCommand request of the check", || {
        http.requests()[before..]
            .iter()
            .any(|seen| seen.path == "/goform/AppCommand.xml")
    })
    .await;
    started.stop();
    let check = timeout(Duration::from_millis(100), started.info_check())
        .await
        .unwrap();
    assert!(
        matches!(check, InfoCheck::Failed(Error::Stopped)),
        "{check:?}"
    );
    assert_eq!(web.requests().len(), 0, "the pages were asked");
}

#[tokio::test]
async fn the_check_of_known_values_comes_before_the_pages() {
    let http = avr_x_receiver().await;
    let web = Fake::start(|_| Reply::Hang).await;
    let info = builder(&http, ReceiverType::AvrX)
        .connect()
        .await
        .unwrap()
        .info()
        .clone();
    let avr = Arc::new(
        Avr::builder("127.0.0.1")
            .http_config(config())
            .web_interface_port(web.port)
            .web_interface_config(config())
            .known(info)
            .connect()
            .await
            .unwrap(),
    );
    let mut started = avr
        .start_with(StartOptions::new().telnet(false))
        .await
        .unwrap();
    // The pages of the web interface do not answer, so the start is not done.
    let check = timeout(Duration::from_millis(300), started.info_check())
        .await
        .expect("the check waited for the pages");
    assert!(matches!(check, InfoCheck::Same));
    assert!(
        timeout(Duration::from_millis(50), started.basic_synced())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn the_waits_can_be_called_again() {
    let (web, telnet) = (web_interface().await, FakeTelnet::start().await);
    let avr = started(&web, &telnet).await;
    let mut started = avr
        .start_with(StartOptions::new().telnet_wait(Duration::from_millis(300)))
        .await
        .unwrap();
    assert_eq!(
        started.basic_synced().await.telnet_sync,
        TelnetSync::NotWaited
    );
    assert_eq!(started.all_synced().await.telnet_sync, TelnetSync::NoAnswer);
    // The result is kept, so this does not wait again.
    let report = timeout(Duration::from_millis(50), started.all_synced())
        .await
        .unwrap();
    assert_eq!(report.telnet_sync, TelnetSync::NoAnswer);
    assert_eq!(
        started.basic_synced().await.telnet_sync,
        TelnetSync::NoAnswer
    );
}
