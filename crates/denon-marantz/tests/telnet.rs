//! The telnet connection against a fake telnet port.

#![cfg(feature = "telnet")]

mod common;

use std::time::Duration;

use common::{FakeTelnet, avr_x_receiver, builder, wait_until};
use denon_marantz::{Avr, Command, Error, TelnetConfig, TelnetEvent, TelnetStatus};
use denon_marantz_proto::telnet::{Event, EventZone, state_queries};
use denon_marantz_proto::{Brand, Power, ReceiverType, Setting, Volume, Zone};
use tokio::time::timeout;

/// Quick reconnects, no spacing between writes, and a keep-alive time that
/// tests do not meet by chance.
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
        background_pause: Duration::ZERO,
    }
}

/// An `Avr` with a telnet connection to `telnet`.
async fn connected(config: TelnetConfig) -> (Avr, common::Fake) {
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(config)
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    (avr, http)
}

async fn status_is(avr: &Avr, wanted: TelnetStatus) {
    let mut status = avr.telnet_status();
    timeout(
        Duration::from_secs(2),
        status.wait_for(|status| *status == wanted),
    )
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {wanted:?}"))
    .unwrap();
}

#[tokio::test]
async fn events_change_the_state() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    status_is(&avr, TelnetStatus::Connected).await;

    telnet.send(b"MV435\rMUON\rZ2ON\rSIGAME\rMSDOLBY  ATMOS\r");
    wait_until("the events", || {
        avr.snapshot().zone(Zone::Main).sound_mode.is_some()
    })
    .await;
    let state = avr.snapshot();
    let main = state.zone(Zone::Main);
    assert_eq!(main.volume.map(Volume::db), Some(-36.5));
    assert_eq!(main.muted, Some(true));
    assert_eq!(main.source.as_deref(), Some("GAME"));
    assert_eq!(main.sound_mode.as_deref(), Some("DOLBY ATMOS"));
    assert_eq!(state.zone(Zone::Zone2).power, Some(Power::On));
}

#[tokio::test]
async fn a_line_split_over_several_reads_is_joined() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    telnet.send(b"MV4");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(avr.snapshot().zone(Zone::Main).volume, None);
    telnet.send(b"35\r");
    wait_until("the volume", || {
        avr.snapshot().zone(Zone::Main).volume.is_some()
    })
    .await;
    assert_eq!(
        avr.snapshot().zone(Zone::Main).volume.map(Volume::db),
        Some(-36.5)
    );
}

#[tokio::test]
async fn garbage_does_not_end_the_connection() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    telnet.send(b"\xff\xfe\r\rXX\rP\rMV435\r");
    wait_until("the volume", || {
        avr.snapshot().zone(Zone::Main).volume.is_some()
    })
    .await;
    assert_eq!(telnet.connections(), 1);
}

#[tokio::test]
async fn changes_and_events_are_published() {
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .connect()
        .await
        .unwrap();
    let mut changes = avr.changes();
    let mut events = avr.events();
    avr.telnet_connect().await.unwrap();

    telnet.send(b"SIGAME\r");
    let change = timeout(Duration::from_secs(2), changes.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        (change.zone, change.setting),
        (Some(Zone::Main), Setting::Source)
    );
    let event = timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        event,
        TelnetEvent {
            zone: EventZone::Main,
            event: Event::Si,
            parameter: "GAME".into(),
        }
    );

    // The same source again is an event but not a change.
    telnet.send(b"SIGAME\rMUON\r");
    let change = timeout(Duration::from_secs(2), changes.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(change.setting, Setting::Muted);
    let event = timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(event.event, Event::Si);
}

#[tokio::test]
async fn repeated_events_from_a_query_are_left_out() {
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .connect()
        .await
        .unwrap();
    let mut events = avr.events();
    avr.telnet_connect().await.unwrap();

    telnet.send(b"SSINFSIGRES I 4K\rSSINFSIGRES I 4K\rSSINFSIGRES I 8K\r");
    for expected in ["INFSIGRES I 4K", "INFSIGRES I 8K"] {
        let event = timeout(Duration::from_secs(2), events.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(&*event.parameter, expected);
    }
    assert!(
        timeout(Duration::from_millis(150), events.recv())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn telnet_and_http_agree_on_values() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    // The same sound mode in the way each source writes it.
    telnet.send(b"MSDOLBY ATMOS\rMV435\r");
    wait_until("the events", || {
        avr.snapshot().zone(Zone::Main).volume.is_some()
    })
    .await;
    let mut changes = avr.changes();
    avr.update().await.unwrap();
    let mut seen = Vec::new();
    while let Ok(change) = changes.try_recv() {
        seen.push((change.zone, change.setting));
    }
    assert!(
        !seen.contains(&(Some(Zone::Main), Setting::SoundMode)),
        "{seen:?}"
    );
    assert!(
        !seen.contains(&(Some(Zone::Main), Setting::Volume)),
        "{seen:?}"
    );
    assert!(
        seen.contains(&(Some(Zone::Main), Setting::Power)),
        "{seen:?}"
    );
}

#[tokio::test]
async fn the_connection_is_opened_again_after_it_is_lost() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    status_is(&avr, TelnetStatus::Connected).await;

    telnet.close();
    status_is(&avr, TelnetStatus::Disconnected).await;
    status_is(&avr, TelnetStatus::Connected).await;
    wait_until("the second connection", || telnet.connections() == 2).await;

    telnet.send(b"MV435\r");
    wait_until("the volume", || {
        avr.snapshot().zone(Zone::Main).volume.is_some()
    })
    .await;
}

#[tokio::test]
async fn the_connection_is_kept_alive_with_a_query() {
    let telnet = FakeTelnet::start().await;
    let config = TelnetConfig {
        keep_alive: Duration::from_millis(100),
        ..telnet_config(telnet.port)
    };
    let (_avr, _http) = connected(config).await;
    wait_until("the first query", || telnet.received().contains("PW?\r")).await;

    // The receiver answers, so the connection counts as alive.
    telnet.send(b"PWON\r");
    wait_until("the second query", || {
        telnet.received().matches("PW?\r").count() == 2
    })
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(telnet.connections(), 1);
}

#[tokio::test]
async fn a_silent_connection_is_replaced() {
    let telnet = FakeTelnet::start().await;
    let config = TelnetConfig {
        keep_alive: Duration::from_millis(100),
        ..telnet_config(telnet.port)
    };
    let (_avr, _http) = connected(config).await;
    wait_until("a new connection", || telnet.connections() >= 2).await;
    assert!(telnet.closed_by_client() >= 1);
}

#[tokio::test]
async fn disconnect_stops_the_task() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    status_is(&avr, TelnetStatus::Connected).await;

    avr.telnet_disconnect().await;
    status_is(&avr, TelnetStatus::Disconnected).await;
    wait_until("the client to close", || telnet.closed_by_client() == 1).await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(
        telnet.connections(),
        1,
        "no new connection after disconnect"
    );

    avr.telnet_connect().await.unwrap();
    status_is(&avr, TelnetStatus::Connected).await;
    wait_until("the second connection", || telnet.connections() == 2).await;
}

#[tokio::test]
async fn connecting_twice_opens_one_connection() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    avr.telnet_connect().await.unwrap();
    avr.telnet_connect().await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(telnet.connections(), 1);
}

#[tokio::test]
async fn dropping_the_avr_closes_the_connection() {
    let telnet = FakeTelnet::start().await;
    let (avr, http) = connected(telnet_config(telnet.port)).await;
    let status = avr.telnet_status();
    wait_until("the connection", || {
        *status.borrow() == TelnetStatus::Connected
    })
    .await;
    drop(avr);
    wait_until("the client to close", || telnet.closed_by_client() == 1).await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(telnet.connections(), 1, "the task is gone");
    drop(http);
}

#[tokio::test]
async fn a_failed_first_connection_is_an_error() {
    let unused = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = unused.local_addr().unwrap().port();
    drop(unused);
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(port))
        .connect()
        .await
        .unwrap();
    let error = avr.telnet_connect().await.unwrap_err();
    assert!(error.is_unreachable(), "{error:?}");
    assert!(
        error.to_string().contains("telnet connect 127.0.0.1"),
        "{error}"
    );
    assert_eq!(*avr.telnet_status().borrow(), TelnetStatus::Disconnected);
}

#[tokio::test]
async fn invalid_settings_are_refused() {
    let http = avr_x_receiver().await;
    let config = TelnetConfig {
        keep_alive: Duration::ZERO,
        ..TelnetConfig::default()
    };
    let error = builder(&http, ReceiverType::AvrX)
        .telnet_config(config)
        .connect()
        .await
        .unwrap_err();
    assert!(matches!(error, Error::InvalidConfig(_)), "{error:?}");
}

/// The lines that the client sent.
fn sent(telnet: &FakeTelnet) -> Vec<String> {
    telnet
        .received()
        .split('\r')
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

#[tokio::test]
async fn the_queries_that_fill_the_state_are_sent_after_a_connect() {
    let telnet = FakeTelnet::start().await;
    let (_avr, _http) = connected(telnet_config(telnet.port)).await;
    let expected: Vec<String> = std::iter::once("ZM?")
        .chain(state_queries(Brand::Denon))
        .map(str::to_owned)
        .collect();
    wait_until("all queries", || sent(&telnet).len() >= expected.len()).await;
    assert_eq!(sent(&telnet), expected);
}

#[tokio::test]
async fn the_queries_are_sent_again_after_a_reconnect() {
    let telnet = FakeTelnet::start().await;
    let (_avr, _http) = connected(telnet_config(telnet.port)).await;
    let count = 1 + state_queries(Brand::Denon).count();
    wait_until("all queries", || sent(&telnet).len() >= count).await;
    telnet.close();
    wait_until("all queries again", || sent(&telnet).len() >= 2 * count).await;
    assert_eq!(telnet.connections(), 2);
}

#[tokio::test]
async fn refresh_sends_the_queries_again() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let count = 1 + state_queries(Brand::Denon).count();
    wait_until("all queries", || sent(&telnet).len() >= count).await;
    avr.telnet_refresh().await.unwrap();
    wait_until("all queries again", || sent(&telnet).len() >= 2 * count).await;
}

#[tokio::test]
async fn a_repeated_event_is_shown_again_after_a_refresh() {
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .connect()
        .await
        .unwrap();
    let mut events = avr.events();
    avr.telnet_connect().await.unwrap();
    telnet.send(b"SSINFSIGRES I 4K\rSSINFSIGRES I 4K\r");
    timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(
        timeout(Duration::from_millis(100), events.recv())
            .await
            .is_err()
    );
    avr.telnet_refresh().await.unwrap();
    telnet.send(b"SSINFSIGRES I 4K\r");
    timeout(Duration::from_secs(2), events.recv())
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn send_writes_the_command_for_the_zone() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    avr.send(Zone::Main, Command::VolumeUp).await.unwrap();
    avr.send(Zone::Zone2, Command::Mute(true)).await.unwrap();
    avr.send(Zone::Main, Command::SelectSource("GAME"))
        .await
        .unwrap();
    wait_until("the commands", || {
        sent(&telnet).iter().any(|line| line == "SIGAME")
    })
    .await;
    let lines = sent(&telnet);
    for command in ["MVUP", "Z2MUON", "SIGAME"] {
        assert!(
            lines.iter().any(|line| line == command),
            "{command} in {lines:?}"
        );
    }
}

#[tokio::test]
async fn commands_go_before_the_remaining_queries() {
    let telnet = FakeTelnet::start().await;
    let config = TelnetConfig {
        query_interval: Duration::from_millis(20),
        ..telnet_config(telnet.port)
    };
    let (avr, _http) = connected(config).await;
    // The queries take about 1.6 s at this speed.
    wait_until("a few queries", || sent(&telnet).len() >= 3).await;
    avr.send(Zone::Main, Command::VolumeDown).await.unwrap();
    wait_until("the command", || {
        sent(&telnet).iter().any(|line| line == "MVDOWN")
    })
    .await;
    let lines = sent(&telnet);
    let position = lines.iter().position(|line| line == "MVDOWN").unwrap();
    assert!(
        position < 20,
        "command at {position}, only {} queries are due",
        lines.len()
    );
    let total = 1 + state_queries(Brand::Denon).count();
    assert!(
        lines.len() < total,
        "the command did not wait for all {total} queries"
    );
}

#[tokio::test]
async fn queries_wait_after_a_command() {
    let telnet = FakeTelnet::start().await;
    let config = TelnetConfig {
        command_interval: Duration::from_millis(10),
        query_interval: Duration::from_millis(20),
        background_pause: Duration::from_millis(300),
        ..telnet_config(telnet.port)
    };
    let (avr, _http) = connected(config).await;
    wait_until("a few queries", || sent(&telnet).len() >= 3).await;
    avr.send(Zone::Main, Command::VolumeUp).await.unwrap();
    wait_until("the command", || {
        sent(&telnet).iter().any(|line| line == "MVUP")
    })
    .await;
    let after_command = sent(&telnet).len();
    tokio::time::sleep(Duration::from_millis(200)).await;
    // The pause is 300 ms, so no query has followed the command yet.
    assert_eq!(sent(&telnet).len(), after_command);
    wait_until("queries to go on", || sent(&telnet).len() > after_command).await;
}

#[tokio::test]
async fn writes_keep_the_spacing() {
    let telnet = FakeTelnet::start().await;
    let config = TelnetConfig {
        command_interval: Duration::from_millis(100),
        ..telnet_config(telnet.port)
    };
    let (avr, _http) = connected(config).await;
    // The first query is sent at once. Wait for the commands to be next in line.
    let started = tokio::time::Instant::now();
    for _ in 0..4 {
        avr.send(Zone::Main, Command::VolumeUp).await.unwrap();
    }
    // Four commands and the queries in between need at least three gaps.
    assert!(
        started.elapsed() >= Duration::from_millis(300),
        "{:?}",
        started.elapsed()
    );
}

#[tokio::test]
async fn nothing_is_sent_while_the_connection_is_not_open() {
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .connect()
        .await
        .unwrap();
    // Not connected yet.
    let error = avr.send(Zone::Main, Command::VolumeUp).await.unwrap_err();
    assert!(error.is_not_connected(), "{error:?}");
    assert!(avr.telnet_refresh().await.unwrap_err().is_not_connected());

    avr.telnet_connect().await.unwrap();
    avr.send(Zone::Main, Command::VolumeUp).await.unwrap();

    // While the task connects again.
    telnet.close();
    status_is(&avr, TelnetStatus::Disconnected).await;
    let error = avr.send(Zone::Main, Command::VolumeUp).await.unwrap_err();
    assert!(error.is_not_connected(), "{error:?}");

    // After a disconnect.
    status_is(&avr, TelnetStatus::Connected).await;
    avr.telnet_disconnect().await;
    let error = avr.send(Zone::Main, Command::VolumeUp).await.unwrap_err();
    assert!(error.is_not_connected(), "{error:?}");
}

#[tokio::test]
async fn invalid_commands_are_refused_before_anything_is_sent() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    wait_until("the queries", || sent(&telnet).len() >= 79).await;
    for text in ["", "A\rMVUP", "A\nB"] {
        let error = avr.send(Zone::Main, Command::Raw(text)).await.unwrap_err();
        assert!(matches!(error, Error::InvalidCommand(_)), "{error:?}");
        assert!(error.is_invalid_input());
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!sent(&telnet).iter().any(|line| line == "MVUP"));
}

#[tokio::test]
async fn a_waiting_command_fails_when_the_connection_ends() {
    let telnet = FakeTelnet::start().await;
    let config = TelnetConfig {
        // Slow writes, so that the command is still waiting when the
        // connection ends.
        command_interval: Duration::from_millis(400),
        ..telnet_config(telnet.port)
    };
    let (avr, _http) = connected(config).await;
    wait_until("the first query", || !sent(&telnet).is_empty()).await;
    let waiting = avr.send(Zone::Main, Command::VolumeUp);
    let (result, ()) = tokio::join!(waiting, async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        telnet.close();
    });
    let error = result.unwrap_err();
    assert!(error.is_not_connected(), "{error:?}");
    // The command is not sent later, after the reconnect.
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        !sent(&telnet).iter().any(|line| line == "MVUP"),
        "{:?}",
        sent(&telnet)
    );
}

#[tokio::test]
async fn queries_keep_their_own_interval() {
    let telnet = FakeTelnet::start().await;
    let config = TelnetConfig {
        query_interval: Duration::from_millis(80),
        ..telnet_config(telnet.port)
    };
    let (_avr, _http) = connected(config).await;
    wait_until("the first query", || !sent(&telnet).is_empty()).await;
    let first = tokio::time::Instant::now();
    wait_until("four queries", || sent(&telnet).len() >= 4).await;
    // Three gaps of 80 ms. The test allows for the clock of the fake.
    assert!(
        first.elapsed() >= Duration::from_millis(200),
        "{:?}",
        first.elapsed()
    );
}
