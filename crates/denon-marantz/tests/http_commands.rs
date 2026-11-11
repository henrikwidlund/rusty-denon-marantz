//! Commands over HTTP, against a fake receiver, and the reads that follow.

#![cfg(feature = "http")]

mod common;

use std::time::Duration;

use common::{AVR_X_INFO, Fake, avr_x_receiver_with_pages, builder, wait_until};
use denon_marantz::{Avr, Command, Error, FollowUpRead, ReceiverType, Zone};

const COMMAND: &str = "/goform/formiPhoneAppDirect.xml?";

/// The commands that the tests send, which the fake receiver answers.
const PAGES: &[(&str, &[u8])] = &[
    ("/goform/formiPhoneAppDirect.xml?ZMON", b""),
    ("/goform/formiPhoneAppDirect.xml?MVUP", b""),
    ("/goform/formiPhoneAppDirect.xml?MV45", b""),
    ("/goform/formiPhoneAppDirect.xml?MSMOVIE", b""),
    ("/goform/formiPhoneAppDirect.xml?SIGAME", b""),
    ("/goform/formiPhoneAppDirect.xml?MUON", b""),
    ("/goform/formiPhoneAppDirect.xml?PSBAS%2050", b""),
];

const fn quick() -> FollowUpRead {
    FollowUpRead {
        quiet: Duration::from_millis(60),
        longest_wait: Duration::from_millis(200),
        retry_every: Duration::from_millis(100),
        retries: 1,
    }
}

async fn receiver() -> Fake {
    avr_x_receiver_with_pages(AVR_X_INFO, PAGES).await
}

async fn connect(fake: &Fake, follow_up: Option<FollowUpRead>) -> Avr {
    builder(fake, ReceiverType::AvrX)
        .follow_up_read(follow_up)
        .connect()
        .await
        .unwrap()
}

fn commands(fake: &Fake) -> Vec<String> {
    fake.requests()
        .into_iter()
        .filter(|seen| seen.method == "GET" && seen.path.starts_with(COMMAND))
        .map(|seen| seen.path[COMMAND.len()..].to_owned())
        .collect()
}

/// The `AppCommand` requests after the first `known` requests.
fn reads_after(fake: &Fake, known: usize) -> Vec<String> {
    fake.requests()
        .into_iter()
        .skip(known)
        .filter(|seen| seen.method == "POST")
        .map(|seen| String::from_utf8_lossy(&seen.body).into_owned())
        .collect()
}

#[tokio::test]
async fn the_typed_controls_work_without_telnet() {
    let fake = receiver().await;
    let avr = connect(&fake, None).await;
    let main = avr.zone(Zone::Main).unwrap();
    main.power_on().await.unwrap();
    main.volume_up().await.unwrap();
    main.set_volume_db(-35.0).await.unwrap();
    main.select_sound_mode("MOVIE").await.unwrap();
    main.select_source("GAME").await.unwrap();
    main.set_mute(true).await.unwrap();
    assert_eq!(
        commands(&fake)[..6],
        ["ZMON", "MVUP", "MV45", "MSMOVIE", "SIGAME", "MUON"]
    );
}

#[tokio::test]
async fn text_with_a_space_is_encoded() {
    let fake = receiver().await;
    let avr = connect(&fake, None).await;
    avr.http_send(Zone::Main, Command::Raw("PSBAS 50"))
        .await
        .unwrap();
    assert_eq!(commands(&fake), ["PSBAS%2050"]);
}

#[tokio::test]
async fn a_query_is_not_sent_over_http() {
    let fake = receiver().await;
    let avr = connect(&fake, None).await;
    let error = avr
        .http_send(Zone::Main, Command::Raw("PW?"))
        .await
        .unwrap_err();
    assert!(error.is_not_connected(), "{error}");
    assert_eq!(commands(&fake), [] as [String; 0]);
}

#[tokio::test]
async fn a_command_that_cannot_be_built_is_refused_before_a_request() {
    let fake = receiver().await;
    let avr = connect(&fake, None).await;
    let error = avr
        .http_send(Zone::Main, Command::Raw("PW\nOFF"))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::InvalidCommand(_)), "{error}");
    assert_eq!(commands(&fake), [] as [String; 0]);
}

#[tokio::test]
async fn a_refused_command_is_an_error() {
    // The fake receiver answers 404 for a command that is not in its list.
    let fake = receiver().await;
    let avr = connect(&fake, None).await;
    let error = avr
        .http_send(Zone::Main, Command::Raw("ZMOFF"))
        .await
        .unwrap_err();
    assert!(error.is_status(), "{error}");
}

#[tokio::test]
async fn a_volume_command_reads_only_the_volume_and_mute() {
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick())).await;
    let known = fake.requests().len();
    avr.zone(Zone::Main).unwrap().volume_up().await.unwrap();
    wait_until("the read", || !reads_after(&fake, known).is_empty()).await;
    tokio::time::sleep(Duration::from_millis(150)).await;
    let reads = reads_after(&fake, known);
    assert_eq!(reads.len(), 1, "{reads:?}");
    assert!(reads[0].contains("GetAllZoneVolume"), "{}", reads[0]);
    assert!(reads[0].contains("GetAllZoneMuteStatus"), "{}", reads[0]);
    assert!(!reads[0].contains("GetAllZonePowerStatus"), "{}", reads[0]);
}

#[tokio::test]
async fn a_key_that_is_held_down_causes_few_reads() {
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick())).await;
    let known = fake.requests().len();
    let main = avr.zone(Zone::Main).unwrap();
    // About 600 ms of commands, one every 5 ms.
    for _ in 0..120 {
        main.volume_up().await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    tokio::time::sleep(Duration::from_millis(300)).await;
    let reads = reads_after(&fake, known).len();
    assert_eq!(commands(&fake).len(), 120);
    // Not one for each command: about one for each 200 ms, and one at the end.
    assert!((2..=6).contains(&reads), "{reads}");
}

#[tokio::test]
async fn late_values_are_read_again_until_one_changes() {
    // The receiver shows a new source, sound mode or power state late, so one
    // read is not enough. The fake receiver always answers the same.
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick())).await;
    avr.update().await.unwrap();
    let known = fake.requests().len();
    let main = avr.zone(Zone::Main).unwrap();
    main.select_sound_mode("MOVIE").await.unwrap();
    main.select_source("GAME").await.unwrap();
    // The first read has both, and the one retry has both again.
    wait_until("the reads", || reads_after(&fake, known).len() >= 4).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let reads = reads_after(&fake, known);
    assert_eq!(reads.len(), 4, "{reads:?}");
    let source = reads
        .iter()
        .filter(|r| r.contains("GetAllZoneSource"))
        .count();
    let mode = reads
        .iter()
        .filter(|r| r.contains("GetSurroundModeStatus"))
        .count();
    assert_eq!((source, mode), (2, 2));
}

#[tokio::test]
async fn the_retries_are_as_many_as_set() {
    let fake = receiver().await;
    let config = FollowUpRead {
        retries: 3,
        ..quick()
    };
    let avr = connect(&fake, Some(config)).await;
    avr.update().await.unwrap();
    let known = fake.requests().len();
    avr.zone(Zone::Main).unwrap().power_on().await.unwrap();
    wait_until("the reads", || reads_after(&fake, known).len() >= 4).await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(reads_after(&fake, known).len(), 4);
}

#[tokio::test]
async fn a_read_that_shows_a_change_ends_the_retries() {
    // The state is empty, so the first read changes it.
    let fake = receiver().await;
    let config = FollowUpRead {
        retries: 3,
        ..quick()
    };
    let avr = connect(&fake, Some(config)).await;
    let known = fake.requests().len();
    avr.zone(Zone::Main).unwrap().power_on().await.unwrap();
    wait_until("the read", || !reads_after(&fake, known).is_empty()).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let all = reads_after(&fake, known);
    assert_eq!(all.len(), 1, "{all:?}");
}

#[tokio::test]
async fn a_volume_command_is_not_read_a_second_time() {
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick())).await;
    let known = fake.requests().len();
    avr.zone(Zone::Main).unwrap().volume_up().await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(reads_after(&fake, known).len(), 1);
}

#[tokio::test]
async fn different_commands_are_read_with_one_wait() {
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick())).await;
    let known = fake.requests().len();
    let main = avr.zone(Zone::Main).unwrap();
    main.power_on().await.unwrap();
    main.select_source("GAME").await.unwrap();
    wait_until("both reads", || reads_after(&fake, known).len() >= 2).await;
    let reads = reads_after(&fake, known);
    assert!(reads.iter().any(|r| r.contains("GetAllZonePowerStatus")));
    assert!(reads.iter().any(|r| r.contains("GetAllZoneSource")));
}

#[tokio::test]
async fn a_command_that_the_read_does_not_cover_causes_no_read() {
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick())).await;
    let known = fake.requests().len();
    avr.http_send(Zone::Main, Command::Raw("PSBAS 50"))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(reads_after(&fake, known), [] as [String; 0]);
}

#[tokio::test]
async fn no_read_follows_if_it_is_turned_off() {
    let fake = receiver().await;
    let avr = connect(&fake, None).await;
    let known = fake.requests().len();
    avr.zone(Zone::Main).unwrap().volume_up().await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(reads_after(&fake, known), [] as [String; 0]);
}

#[tokio::test]
async fn the_follow_up_settings_are_checked() {
    let fake = receiver().await;
    let bad = FollowUpRead {
        quiet: Duration::ZERO,
        longest_wait: Duration::from_secs(1),
        retry_every: Duration::from_secs(1),
        retries: 1,
    };
    let error = builder(&fake, ReceiverType::AvrX)
        .follow_up_read(Some(bad))
        .connect()
        .await
        .unwrap_err();
    assert!(error.is_invalid_config(), "{error}");
}
