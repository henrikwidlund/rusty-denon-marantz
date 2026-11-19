//! The HEOS player of an `Avr` against a fake HEOS device.

#![cfg(all(feature = "heos", feature = "telnet"))]

mod common;
#[path = "../../denon-marantz-heos/tests/common/mod.rs"]
mod heos_fake;

use std::sync::Arc;
use std::time::Duration;

use common::{Fake as FakeReceiver, avr_x_receiver, avr_x_receiver_with_info, builder};
use denon_marantz::heos::{Change, Error as HeosError, HeosConfig, HeosStatus, PlayerState};
use denon_marantz::{
    Avr, Error, HeosFailure, HeosPlayerStatus, HeosStart, ReceiverType, StartOptions,
};
use heos_fake::{Fake, IP, PID, Reply, answer_lines, config, counter, event, from_data, name};
use tokio::sync::watch;

/// The receiver without HEOS.
const NO_HEOS_INFO: &[u8] = b"<Device_Info><CommApiVers>0300</CommApiVers>\
    <ModelName>AVR-X2100W</ModelName><DeviceZones>2</DeviceZones></Device_Info>";

/// Answers from the recorded data, with the player at the address of the
/// fake receiver.
fn player_here(line: &str) -> Reply {
    Reply::Lines(
        answer_lines(name(line))
            .into_iter()
            .map(|line| line.replace(IP, "127.0.0.1"))
            .collect(),
    )
}

/// Answers from the recorded data, with a player that has neither the
/// address nor the serial number of the receiver.
fn other_player(line: &str) -> Reply {
    Reply::Lines(
        answer_lines(name(line))
            .into_iter()
            .map(|line| line.replace(r#""serial": "XXX""#, r#""serial": "YYY""#))
            .collect(),
    )
}

/// An `Avr` for a receiver with HEOS, with `heos` as its HEOS device.
async fn avr_with(heos: &Fake) -> (Arc<Avr>, FakeReceiver) {
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .heos_config(config(heos.port))
        .connect()
        .await
        .unwrap();
    (Arc::new(avr), http)
}

/// A start without telnet and without the signal pages.
const fn options() -> StartOptions {
    StartOptions::new().telnet(false).signal_pages(false)
}

/// Waits until the status is `wanted`. Fails the test after two seconds.
async fn status_is(status: &mut watch::Receiver<HeosPlayerStatus>, wanted: HeosPlayerStatus) {
    let wait = async { status.wait_for(|now| *now == wanted).await.is_ok() };
    let reached = tokio::time::timeout(Duration::from_secs(2), wait).await;
    assert_eq!(reached, Ok(true), "{wanted:?}, now {:?}", *status.borrow());
}

#[tokio::test]
async fn the_start_opens_heos_and_finds_the_player() {
    let heos = Fake::start(player_here).await;
    let (avr, _http) = avr_with(&heos).await;
    assert!(avr.info().has_heos());
    let mut status = avr.watch_heos_status();
    assert_eq!(*status.borrow(), HeosPlayerStatus::Disconnected);
    let mut started = avr.start_with(options()).await.unwrap();
    let report = started.basic_synced().await;
    assert_eq!(report.heos, HeosStart::Started);
    status_is(&mut status, HeosPlayerStatus::Ready(PID)).await;
    let player = avr.heos().unwrap();
    assert_eq!(player.pid(), PID);
    assert!(player.state().is_some());
    assert_eq!(player.with_state(PlayerState::pid), Some(PID));
    player.play_next().await.unwrap();
    assert_eq!(heos.count("player/play_next"), 1);
}

#[tokio::test]
async fn the_player_is_found_when_the_list_has_it() {
    let calls = counter();
    let heos = Fake::start(move |line| {
        if name(line) == "player/get_players" && calls() == 0 {
            other_player(line)
        } else {
            player_here(line)
        }
    })
    .await;
    let (avr, _http) = avr_with(&heos).await;
    let mut status = avr.watch_heos_status();
    avr.heos_connect().await.unwrap();
    status_is(&mut status, HeosPlayerStatus::WaitingForPlayer).await;
    assert!(avr.heos().is_none());
    heos.push(event("players_changed", ""));
    status_is(&mut status, HeosPlayerStatus::Ready(PID)).await;
    assert!(avr.heos().is_some());
}

#[tokio::test]
async fn the_player_is_found_by_the_serial_number() {
    // The recorded player has another address but the same serial number.
    let heos = Fake::start(from_data).await;
    let (avr, _http) = avr_with(&heos).await;
    assert_eq!(avr.info().serial_number.as_deref(), Some("XXX"));
    let mut status = avr.watch_heos_status();
    avr.heos_connect().await.unwrap();
    status_is(&mut status, HeosPlayerStatus::Ready(PID)).await;
}

#[tokio::test]
async fn changes_come_from_each_connection() {
    let heos = Fake::start(player_here).await;
    let (avr, _http) = avr_with(&heos).await;
    let mut changes = avr.heos_changes();
    let mut status = avr.watch_heos_status();
    for _ in 0..2 {
        avr.heos_connect().await.unwrap();
        status_is(&mut status, HeosPlayerStatus::Ready(PID)).await;
        heos.push(event("player_state_changed", "pid=1234567&state=play"));
        loop {
            let change = tokio::time::timeout(Duration::from_secs(2), changes.recv())
                .await
                .unwrap()
                .unwrap();
            if change == Change::PlayState(PID) {
                break;
            }
        }
        avr.heos_disconnect().await;
    }
}

#[tokio::test]
async fn a_disconnect_also_closes_a_kept_client() {
    let heos = Fake::start(player_here).await;
    let (avr, _http) = avr_with(&heos).await;
    let mut status = avr.watch_heos_status();
    avr.heos_connect().await.unwrap();
    status_is(&mut status, HeosPlayerStatus::Ready(PID)).await;
    let client = avr.heos().unwrap().client().unwrap();
    avr.heos_disconnect().await;
    assert!(matches!(
        client.play_next(PID).await,
        Err(HeosError::NotConnected)
    ));
    let mut connection = client.watch_status();
    let closed = connection.wait_for(|now| *now == HeosStatus::Disconnected);
    tokio::time::timeout(Duration::from_secs(2), closed)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn a_lost_connection_is_opened_again() {
    let heos = Fake::start(player_here).await;
    let (avr, _http) = avr_with(&heos).await;
    let mut status = avr.watch_heos_status();
    avr.heos_connect().await.unwrap();
    status_is(&mut status, HeosPlayerStatus::Ready(PID)).await;
    heos.close_all();
    status_is(&mut status, HeosPlayerStatus::Connecting).await;
    status_is(&mut status, HeosPlayerStatus::Ready(PID)).await;
}

#[tokio::test]
async fn a_second_connect_does_nothing() {
    let heos = Fake::start(player_here).await;
    let (avr, _http) = avr_with(&heos).await;
    avr.heos_connect().await.unwrap();
    let mut started = avr.start_with(options()).await.unwrap();
    let report = started.basic_synced().await;
    assert!(
        matches!(report.heos, HeosStart::AlreadyRunning),
        "{:?}",
        report.heos
    );
    avr.heos_connect().await.unwrap();
    // Two connections: commands and events.
    assert_eq!(heos.connections(), 2);
}

#[tokio::test]
async fn the_start_can_leave_heos_closed() {
    let heos = Fake::start(player_here).await;
    let (avr, _http) = avr_with(&heos).await;
    let mut started = avr.start_with(options().heos(false)).await.unwrap();
    let report = started.basic_synced().await;
    assert!(
        matches!(report.heos, HeosStart::Disabled),
        "{:?}",
        report.heos
    );
    assert_eq!(heos.connections(), 0);
    assert_eq!(
        *avr.watch_heos_status().borrow(),
        HeosPlayerStatus::Disconnected
    );
}

#[tokio::test]
async fn a_receiver_without_heos_is_not_asked() {
    let http = avr_x_receiver_with_info(NO_HEOS_INFO).await;
    let heos = Fake::start(player_here).await;
    let avr = Arc::new(
        builder(&http, ReceiverType::AvrX)
            .heos_config(config(heos.port))
            .connect()
            .await
            .unwrap(),
    );
    assert!(!avr.info().has_heos());
    assert_eq!(
        *avr.watch_heos_status().borrow(),
        HeosPlayerStatus::NotSupported
    );
    let mut started = avr.start_with(options()).await.unwrap();
    let report = started.basic_synced().await;
    assert!(
        matches!(report.heos, HeosStart::NotSupported),
        "{:?}",
        report.heos
    );
    let error = avr.heos_connect().await.unwrap_err();
    assert!(matches!(error, Error::HeosNotSupported), "{error:?}");
    assert!(error.is_unsupported());
    assert_eq!(heos.connections(), 0);
}

#[tokio::test]
async fn a_dropped_connect_leaves_heos_disconnected() {
    let heos = Fake::start(|line| match name(line) {
        "player/get_players" => Reply::Hang,
        _ => player_here(line),
    })
    .await;
    let (avr, _http) = avr_with(&heos).await;
    let mut status = avr.watch_heos_status();
    let connect = tokio::time::timeout(Duration::from_millis(100), avr.heos_connect()).await;
    assert!(connect.is_err(), "{connect:?}");
    assert_eq!(*status.borrow_and_update(), HeosPlayerStatus::Disconnected);
    assert!(avr.heos().is_none());
}

#[tokio::test]
async fn a_failed_start_shows_in_the_status_and_keeps_the_error() {
    // No fake listens on the HEOS port.
    let http = avr_x_receiver().await;
    let avr = Arc::new(builder(&http, ReceiverType::AvrX).connect().await.unwrap());
    let mut status = avr.watch_heos_status();
    let failed = HeosPlayerStatus::Failed(HeosFailure::Refused);
    // The error is there as soon as the status says that it failed.
    let watcher = {
        let avr = Arc::clone(&avr);
        let mut status = avr.watch_heos_status();
        tokio::spawn(async move {
            let _ = status.wait_for(|now| *now == failed).await;
            avr.heos_start_error().is_some()
        })
    };
    let mut started = avr.start_with(options()).await.unwrap();
    assert_eq!(started.basic_synced().await.heos, HeosStart::Started);
    status_is(&mut status, failed).await;
    assert!(watcher.await.unwrap());
    let error = avr.heos_start_error().unwrap();
    assert!(
        matches!(*error, Error::Heos(HeosError::Network { .. })),
        "{error:?}"
    );
}

#[tokio::test]
async fn the_start_does_not_wait_for_heos_that_never_answers() {
    let heos = Fake::start(|line| match name(line) {
        "player/get_players" => Reply::Hang,
        _ => player_here(line),
    })
    .await;
    let http = avr_x_receiver().await;
    let avr = Arc::new(
        builder(&http, ReceiverType::AvrX)
            .heos_config(HeosConfig {
                command_timeout: Duration::from_secs(3),
                ..config(heos.port)
            })
            .connect()
            .await
            .unwrap(),
    );
    let mut status = avr.watch_heos_status();
    let mut started = avr.start_with(options()).await.unwrap();
    // The status is set before the start returns.
    assert_eq!(*status.borrow(), HeosPlayerStatus::Connecting);
    let report = tokio::time::timeout(Duration::from_secs(1), started.basic_synced())
        .await
        .expect("the start waited for HEOS");
    assert_eq!(report.heos, HeosStart::Started);
    assert_eq!(*status.borrow(), HeosPlayerStatus::Connecting);
    let failed = HeosPlayerStatus::Failed(HeosFailure::Timeout);
    let wait = status.wait_for(|now| *now == failed);
    assert!(
        tokio::time::timeout(Duration::from_secs(5), wait)
            .await
            .is_ok()
    );
    assert!(avr.heos_start_error().is_some());
}

#[tokio::test]
async fn the_connect_of_the_start_does_not_keep_the_avr() {
    let heos = Fake::start(|line| match name(line) {
        "player/get_players" => Reply::Hang,
        _ => player_here(line),
    })
    .await;
    let (avr, _http) = avr_with(&heos).await;
    let mut started = avr.start_with(options()).await.unwrap();
    started.basic_synced().await;
    let weak = Arc::downgrade(&avr);
    drop(started);
    drop(avr);
    assert!(weak.upgrade().is_none());
}

#[tokio::test]
async fn a_failed_connect_returns_its_error() {
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX).connect().await.unwrap();
    let error = avr.heos_connect().await.unwrap_err();
    assert!(matches!(error, Error::Heos(_)), "{error:?}");
    assert_eq!(
        *avr.watch_heos_status().borrow(),
        HeosPlayerStatus::Failed(HeosFailure::Refused)
    );
    assert!(avr.heos_start_error().is_none());
}

#[tokio::test]
async fn a_disconnect_closes_heos_for_the_player_too() {
    let heos = Fake::start(player_here).await;
    let (avr, _http) = avr_with(&heos).await;
    let mut status = avr.watch_heos_status();
    avr.heos_connect().await.unwrap();
    status_is(&mut status, HeosPlayerStatus::Ready(PID)).await;
    let player = avr.heos().unwrap();
    avr.heos_disconnect().await;
    assert_eq!(*status.borrow(), HeosPlayerStatus::Disconnected);
    assert!(avr.heos().is_none());
    assert!(player.client().is_none());
    assert!(matches!(
        player.play_next().await,
        Err(HeosError::NotConnected)
    ));
    // It can be opened again.
    avr.heos_connect().await.unwrap();
    status_is(&mut status, HeosPlayerStatus::Ready(PID)).await;
}

#[tokio::test]
async fn dropping_the_avr_closes_heos() {
    let heos = Fake::start(player_here).await;
    let (avr, _http) = avr_with(&heos).await;
    let mut status = avr.watch_heos_status();
    avr.heos_connect().await.unwrap();
    status_is(&mut status, HeosPlayerStatus::Ready(PID)).await;
    let player = avr.heos().unwrap();
    drop(avr);
    assert!(player.client().is_none());
    assert_eq!(*status.borrow(), HeosPlayerStatus::Disconnected);
}
