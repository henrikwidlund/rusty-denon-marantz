//! The telnet connection against a fake telnet port.

#![cfg(feature = "telnet")]

mod common;

use std::time::Duration;

use common::{
    AVR_X_DIRAC_INFO, FakeTelnet, avr_x_receiver, avr_x_receiver_with_info, builder, wait_until,
};
use denon_marantz::{Avr, Command, Error, TelnetConfig, TelnetEvent, TelnetStatus, ToneBlocker};
use denon_marantz_proto::telnet::{Event, EventZone, state_queries};
use denon_marantz_proto::{
    AudioRestorer, Brand, ChannelLevel, DiracFilter, DynamicVolume, MultiEq, Power, ReceiverType,
    ReferenceLevelOffset, RoomSize, Setting, ToneLevel, Volume, Zone,
};
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
        query_quiet: Duration::ZERO,
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

#[tokio::test]
async fn zone_methods_send_the_commands_of_the_zone() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let main = avr.zone(Zone::Main).unwrap();
    let zone2 = avr.zone(Zone::Zone2).unwrap();
    assert_eq!(zone2.zone(), Zone::Zone2);
    main.power_on().await.unwrap();
    main.set_volume_db(-36.5).await.unwrap();
    main.select_sound_mode("MOVIE").await.unwrap();
    zone2.set_mute(true).await.unwrap();
    zone2.volume_up().await.unwrap();
    zone2.select_source("GAME").await.unwrap();
    zone2.power_standby().await.unwrap();
    wait_until("the commands", || sent(&telnet).len() >= 7).await;
    let commands: Vec<String> = sent(&telnet)
        .into_iter()
        .filter(|line| !line.ends_with('?'))
        .collect();
    assert_eq!(
        commands,
        [
            "ZMON", "MV435", "MSMOVIE", "Z2MUON", "Z2UP", "Z2GAME", "Z2OFF"
        ]
    );
}

#[tokio::test]
async fn zone_methods_refuse_what_cannot_work() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    // The receiver of the test has two zones.
    let error = avr.zone(Zone::Zone3).unwrap_err();
    assert!(matches!(error, Error::NoSuchZone(Zone::Zone3)), "{error:?}");
    assert!(error.is_invalid_input());
    let zone2 = avr.zone(Zone::Zone2).unwrap();
    let error = zone2.select_sound_mode("MOVIE").await.unwrap_err();
    assert!(
        matches!(error, Error::NoSoundMode(Zone::Zone2)),
        "{error:?}"
    );
    for db in [18.5, f32::NAN] {
        let error = zone2.set_volume_db(db).await.unwrap_err();
        assert!(matches!(error, Error::VolumeOutOfRange(_)), "{error:?}");
        assert!(error.is_invalid_input());
    }
    let error = zone2.select_source("").await.unwrap_err();
    assert!(matches!(error, Error::InvalidCommand(_)), "{error:?}");
    // Only the queries after the connect went out.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        sent(&telnet).iter().all(|line| line.ends_with('?')),
        "{:?}",
        sent(&telnet)
    );
}

/// The commands that the client sent, without the queries.
fn commands_sent(telnet: &FakeTelnet) -> Vec<String> {
    sent(telnet)
        .into_iter()
        .filter(|line| !line.ends_with('?'))
        .collect()
}

#[tokio::test]
async fn tone_events_change_the_state() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    telnet.send(b"PSTONE CTRL ON\rPSBAS 53\rPSTRE 44\r");
    wait_until("the tone values", || {
        avr.snapshot().zone(Zone::Main).treble.is_some()
    })
    .await;
    let state = avr.snapshot();
    assert_eq!(state.device().tone_control, Some(true));
    assert_eq!(state.zone(Zone::Main).bass, ToneLevel::from_db(3));
    assert_eq!(state.zone(Zone::Main).treble, Some(ToneLevel::MIN));
}

#[tokio::test]
async fn tone_methods_turn_tone_control_on_first() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let tone = avr.tone();
    // The state does not say that tone control is on.
    tone.set_bass_db(-6).await.unwrap();
    tone.treble_up().await.unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 4).await;
    assert_eq!(
        commands_sent(&telnet),
        ["PSTONE CTRL ON", "PSBAS 44", "PSTONE CTRL ON", "PSTRE UP"]
    );
    // The receiver says that it is on. Then no switch command goes first.
    telnet.send(b"PSTONE CTRL ON\r");
    wait_until("tone control on", || {
        avr.snapshot().device().tone_control == Some(true)
    })
    .await;
    tone.bass_down().await.unwrap();
    tone.set_treble(ToneLevel::MAX).await.unwrap();
    tone.set_enabled(false).await.unwrap();
    wait_until("more commands", || commands_sent(&telnet).len() >= 7).await;
    assert_eq!(
        commands_sent(&telnet)[4..],
        ["PSBAS DOWN", "PSTRE 56", "PSTONE CTRL OFF"]
    );
}

#[tokio::test]
async fn tone_steps_stop_at_the_ends() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    telnet.send(b"PSTONE CTRL ON\rPSBAS 56\rPSTRE 44\r");
    wait_until("the tone values", || {
        avr.snapshot().zone(Zone::Main).treble.is_some()
    })
    .await;
    let tone = avr.tone();
    tone.bass_up().await.unwrap();
    tone.treble_down().await.unwrap();
    // These go out. The last one shows that nothing came before it.
    tone.bass_down().await.unwrap();
    wait_until("the command", || !commands_sent(&telnet).is_empty()).await;
    assert_eq!(commands_sent(&telnet), ["PSBAS DOWN"]);
}

#[tokio::test]
async fn a_tone_level_out_of_range_is_refused() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    for db in [-7, 7, i8::MIN, i8::MAX] {
        let error = avr.tone().set_bass_db(db).await.unwrap_err();
        assert!(
            matches!(error, Error::ToneOutOfRange(value) if value == db),
            "{error:?}"
        );
        assert!(error.is_invalid_input());
        let error = avr.tone().set_treble_db(db).await.unwrap_err();
        assert!(matches!(error, Error::ToneOutOfRange(_)), "{error:?}");
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(commands_sent(&telnet).is_empty(), "{:?}", sent(&telnet));
}

#[tokio::test]
async fn audyssey_methods_send_their_commands() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let audyssey = avr.audyssey();
    audyssey.set_dynamic_eq(true).await.unwrap();
    audyssey.set_multi_eq(MultiEq::Reference).await.unwrap();
    audyssey
        .set_reference_level_offset(ReferenceLevelOffset::Db5)
        .await
        .unwrap();
    audyssey
        .set_dynamic_volume(DynamicVolume::Medium)
        .await
        .unwrap();
    audyssey.set_lfc(false).await.unwrap();
    audyssey.set_containment_level(3).await.unwrap();
    audyssey.containment_amount_up().await.unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 7).await;
    assert_eq!(
        commands_sent(&telnet),
        [
            "PSDYNEQ ON",
            "PSMULTEQ:AUDYSSEY",
            "PSREFLEV 5",
            "PSDYNVOL MED",
            "PSLFC OFF",
            "PSCNTAMT 03",
            "PSCNTAMT UP"
        ]
    );
}

#[tokio::test]
async fn sound_methods_send_their_commands() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let sound = avr.sound();
    sound.set_room_size(RoomSize::Large).await.unwrap();
    sound.set_audio_restorer(AudioRestorer::Off).await.unwrap();
    sound.set_graphic_eq(true).await.unwrap();
    sound.set_cinema_eq(false).await.unwrap();
    sound.set_audio_delay(5).await.unwrap();
    sound.set_imax_high_pass(80).await.unwrap();
    let level = ChannelLevel::from_half_db(-1).unwrap();
    sound.set_subwoofer_level(2, level).await.unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 7).await;
    assert_eq!(
        commands_sent(&telnet),
        [
            "PSRSZ L",
            "PSRSTR OFF",
            "PSGEQ ON",
            "PSCINEMA EQ.OFF",
            "PSDELAY 005",
            "PSIMAXHPF 080",
            "PSSWL2 495"
        ]
    );
}

#[tokio::test]
async fn sound_methods_refuse_numbers_out_of_range() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let sound = avr.sound();
    let level = ChannelLevel::from_half_db(0).unwrap();
    let errors = [
        sound.set_audio_delay(501).await.unwrap_err(),
        sound.set_center_width(8).await.unwrap_err(),
        sound.set_imax_high_pass(70).await.unwrap_err(),
        sound.set_subwoofer_level(5, level).await.unwrap_err(),
    ];
    for error in errors {
        assert!(matches!(error, Error::InvalidCommand(_)), "{error:?}");
        assert!(error.is_invalid_input());
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(commands_sent(&telnet).is_empty(), "{:?}", sent(&telnet));
}

#[tokio::test]
async fn audyssey_methods_refuse_what_cannot_work() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let audyssey = avr.audyssey();
    // The offset needs Dynamic EQ, and the state says that it is off.
    telnet.send(b"PSDYNEQ OFF\rPSCNTAMT 07\r");
    wait_until("the state", || {
        let state = avr.snapshot();
        state.device().dynamic_eq.is_some() && state.device().containment_amount.is_some()
    })
    .await;
    let error = audyssey
        .set_reference_level_offset(ReferenceLevelOffset::Db0)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::DynamicEqOff), "{error:?}");
    assert!(error.is_invalid_input());
    for level in [0, 8, u8::MAX] {
        let error = audyssey.set_containment_level(level).await.unwrap_err();
        assert!(
            matches!(error, Error::ContainmentOutOfRange(value) if value == level),
            "{error:?}"
        );
    }
    // Steps stop at the highest amount. The step down goes out.
    audyssey.containment_amount_up().await.unwrap();
    audyssey.containment_amount_down().await.unwrap();
    wait_until("the command", || !commands_sent(&telnet).is_empty()).await;
    assert_eq!(commands_sent(&telnet), ["PSCNTAMT DOWN"]);
}

#[tokio::test]
async fn tone_control_is_not_turned_on_when_the_receiver_would_ignore_it() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let tone = avr.tone();
    telnet.send(b"PSDIRAC 2\r");
    wait_until("the filter", || {
        avr.snapshot().device().dirac_filter.is_some()
    })
    .await;
    for error in [
        tone.set_enabled(true).await.unwrap_err(),
        tone.set_bass_db(3).await.unwrap_err(),
    ] {
        assert!(
            matches!(error, Error::ToneControlBlocked(ToneBlocker::DiracFilter)),
            "{error:?}"
        );
        assert!(error.is_invalid_input());
    }
    telnet.send(b"PSDIRAC OFF\rPSDYNEQ ON\r");
    wait_until("dynamic eq", || {
        avr.snapshot().device().dynamic_eq == Some(true)
    })
    .await;
    let error = tone.treble_up().await.unwrap_err();
    assert!(
        matches!(error, Error::ToneControlBlocked(ToneBlocker::DynamicEq)),
        "{error:?}"
    );
    // Turning it off is not blocked.
    tone.set_enabled(false).await.unwrap();
    wait_until("the command", || !commands_sent(&telnet).is_empty()).await;
    assert_eq!(commands_sent(&telnet), ["PSTONE CTRL OFF"]);
    // If the state says that tone control is on, the levels work.
    telnet.send(b"PSTONE CTRL ON\r");
    wait_until("tone control on", || {
        avr.snapshot().device().tone_control == Some(true)
    })
    .await;
    tone.set_bass_db(3).await.unwrap();
    wait_until("the level", || commands_sent(&telnet).len() >= 2).await;
    assert_eq!(commands_sent(&telnet)[1], "PSBAS 53");
}

#[tokio::test]
async fn dirac_needs_a_receiver_that_supports_it() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let error = avr.dirac().unwrap_err();
    assert!(matches!(error, Error::DiracNotSupported), "{error:?}");
    assert!(error.is_invalid_input());

    let http = avr_x_receiver_with_info(AVR_X_DIRAC_INFO).await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    avr.dirac()
        .unwrap()
        .set_filter(DiracFilter::Slot2)
        .await
        .unwrap();
    wait_until("the command", || !commands_sent(&telnet).is_empty()).await;
    assert_eq!(commands_sent(&telnet), ["PSDIRAC 2"]);
}

#[tokio::test]
async fn dirac_slot_events_fill_the_slots() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let mut changes = avr.changes();
    telnet.send(b"OPDLSSL2NAME Dirac ART      \rOPDLSSL2IMP ON\rOPDLSSL2TYP ART\rOPDLS END\r");
    wait_until("the slot", || {
        avr.snapshot().device().dirac_slots[1].kind.is_some()
    })
    .await;
    let slot = avr.snapshot().device().dirac_slots[1].clone();
    assert_eq!(slot.name.as_deref(), Some("Dirac ART"));
    assert_eq!(slot.imported, Some(true));
    assert_eq!(slot.kind.as_deref(), Some("ART"));
    let change = changes.recv().await.unwrap();
    assert_eq!(change.setting, Setting::DiracSlot(2));
}

#[tokio::test]
async fn source_events_fill_the_source_list() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let mut changes = avr.changes();
    telnet.send(
        b"SSFUNMPLAY Apple TV   \rSSFUNTV TV Audio\rSSFUN END\rSSSODMPLAY USE\rSSSODTV DEL\rSSSOD END\r",
    );
    wait_until("the sources", || {
        avr.snapshot()
            .device()
            .sources
            .get("TV")
            .is_some_and(|source| source.in_use.is_some())
    })
    .await;
    let state = avr.snapshot();
    let sources = &state.device().sources;
    assert_eq!(sources.name("MPLAY"), Some("Apple TV"));
    assert_eq!(sources.name("TV"), Some("TV Audio"));
    assert_eq!(sources.get("MPLAY").unwrap().in_use, Some(true));
    assert_eq!(sources.get("TV").unwrap().in_use, Some(false));
    let change = changes.recv().await.unwrap();
    assert_eq!(change.setting, Setting::Sources);
}
