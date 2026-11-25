//! Commands over HTTP, against a fake receiver, and the reads that follow.

#![cfg(feature = "http")]

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{
    AUDIO_INFO_ONLY_INFO, AVR_X_INFO, AVR_X_OUTPUT_SETTINGS_INFO, AVR_X_SETTINGS_INFO, Fake, Reply,
    avr_x_receiver_with_pages, bodies_after, builder, config, contains, friendly_name_answer,
    is_friendly_name_request, output_settings_item, quick_follow_up, update_answer_for, wait_until,
};
use denon_marantz::proto::{HdmiOutput, SoundSettingValue, TUNER_STATUS_PATH, TunerCommand};
use denon_marantz::{
    Avr, Command, Error, FollowUpRead, HttpConfig, Pacing, ReceiverType, Volume, Zone,
};

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
    ("/goform/formiPhoneAppDirect.xml?PSBAS%20UP", b""),
    ("/goform/formiPhoneAppDirect.xml?Z2PSBAS%20UP", b""),
    ("/goform/formiPhoneAppDirect.xml?PSDYNEQ%20ON", b""),
    ("/goform/formiPhoneAppDirect.xml?TFANUP", b""),
];

/// A status page of the tuner on FM 87.50 MHz.
const TUNER_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/Own-formTuner_TunerXml-fm.xml");

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
    bodies_after(fake, known, |seen| seen.method == "POST")
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
    assert!(matches!(error, Error::QueryNeedsTelnet), "{error}");
    assert!(error.is_unsupported() && !error.is_not_connected());
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
    assert!(
        matches!(
            error,
            Error::InvalidCommand(denon_marantz_proto::Error::InvalidCommandText)
        ),
        "{error}"
    );
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

/// The answers to the settings commands of
/// [`a_settings_command_reads_its_part_of_the_settings`] and
/// [`settings_reads_after`].
const SETTINGS_PAGES: &[(&str, &[u8])] = &[
    ("/goform/formiPhoneAppDirect.xml?PSCINEMA%20EQ.OFF", b""),
    ("/goform/formiPhoneAppDirect.xml?PSDELAY%20100", b""),
    ("/goform/formiPhoneAppDirect.xml?MSMOVIE", b""),
    ("/goform/formiPhoneAppDirect.xml?SIGAME", b""),
    ("/goform/formiPhoneAppDirect.xml?Z2GAME", b""),
];

#[tokio::test]
async fn a_settings_command_reads_its_part_of_the_settings() {
    let fake = avr_x_receiver_with_pages(AVR_X_SETTINGS_INFO, SETTINGS_PAGES).await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    let settings_after = |known| {
        bodies_after(&fake, known, |seen| {
            seen.path == "/goform/AppCommand0300.xml"
                && (contains(&seen.body, "GetBTTX") || contains(&seen.body, "GetRestorerMode"))
        })
    };
    let known = fake.requests().len();
    avr.http_send(
        Zone::Main,
        Command::SetSoundSetting(SoundSettingValue::CinemaEq(false)),
    )
    .await
    .unwrap();
    wait_until("the fast settings", || !settings_after(known).is_empty()).await;
    let bodies = settings_after(known);
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    assert!(bodies[0].contains("GetBTTX"), "{}", bodies[0]);
    // The fake receiver still answers cinema EQ on.
    wait_until("the value", || {
        avr.snapshot().device().sound_settings.cinema_eq == Some(true)
    })
    .await;
    let known = fake.requests().len();
    avr.http_send(
        Zone::Main,
        Command::SetSoundSetting(SoundSettingValue::AudioDelay(100)),
    )
    .await
    .unwrap();
    wait_until("the slow settings", || !settings_after(known).is_empty()).await;
    let bodies = settings_after(known);
    assert_eq!(bodies.len(), 1, "{bodies:?}");
    assert!(bodies[0].contains("GetRestorerMode"), "{}", bodies[0]);
}

/// Sends `command` to `zone` and checks the number of fast and slow
/// settings reads that follow. The fake receiver keeps its sound mode and
/// source, so the value that `marker` reads is read again once.
async fn settings_reads_after(
    zone: Zone,
    command: Command<'_>,
    marker: &str,
    (fast, slow): (usize, usize),
) {
    let fake = avr_x_receiver_with_pages(AVR_X_SETTINGS_INFO, SETTINGS_PAGES).await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    avr.update().await.unwrap();
    let known = fake.requests().len();
    avr.http_send(zone, command).await.unwrap();
    let count = |path: &str, text: &str| {
        bodies_after(&fake, known, |seen| {
            seen.path == path && contains(&seen.body, text)
        })
        .len()
    };
    let counts = || {
        (
            count("/goform/AppCommand.xml", marker),
            count("/goform/AppCommand0300.xml", "GetBTTX"),
            count("/goform/AppCommand0300.xml", "GetRestorerMode"),
        )
    };
    wait_until("the reads", || {
        let (reads, fast_now, slow_now) = counts();
        reads >= 2 && fast_now >= fast && slow_now >= slow
    })
    .await;
    // No more reads come.
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(counts(), (2, fast, slow));
}

#[tokio::test]
async fn a_sound_mode_command_reads_the_fast_settings_again_with_the_sound_mode() {
    let command = Command::SelectSoundMode("MOVIE");
    settings_reads_after(Zone::Main, command, "GetSurroundModeStatus", (2, 0)).await;
}

#[tokio::test]
async fn a_source_command_reads_all_settings_and_the_fast_ones_again_with_the_source() {
    let command = Command::SelectSource("GAME");
    settings_reads_after(Zone::Main, command, "GetAllZoneSource", (2, 1)).await;
}

#[tokio::test]
async fn a_source_command_of_zone_2_does_not_read_the_settings() {
    let command = Command::SelectSource("GAME");
    settings_reads_after(Zone::Zone2, command, "GetAllZoneSource", (0, 0)).await;
}

#[tokio::test]
async fn a_volume_command_reads_only_the_volume_and_mute() {
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
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
    let avr = connect(&fake, Some(quick_follow_up())).await;
    let known = fake.requests().len();
    let main = avr.zone(Zone::Main).unwrap();
    // At least 600 ms of commands, one every 5 ms.
    let begin = std::time::Instant::now();
    for _ in 0..120 {
        main.volume_up().await.unwrap();
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    let elapsed = begin.elapsed();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let reads = reads_after(&fake, known).len();
    assert_eq!(commands(&fake).len(), 120);
    // Not one for each command: about one for each 200 ms, and one at the end,
    // whatever the speed of the machine.
    let most = elapsed.as_millis() / 200 + 3;
    assert!(
        reads >= 2 && reads as u128 <= most && reads < 30,
        "{reads} {elapsed:?}"
    );
}

#[tokio::test]
async fn late_values_are_read_again_until_one_changes() {
    // The receiver shows a new source, sound mode or power state late, so one
    // read is not enough. The fake receiver always answers the same.
    let fake = receiver().await;
    // The two commands must come within `quiet`, also on a slow machine.
    let config = FollowUpRead {
        quiet: Duration::from_millis(400),
        longest_wait: Duration::from_secs(2),
        ..quick_follow_up()
    };
    let avr = connect(&fake, Some(config)).await;
    avr.update().await.unwrap();
    let known = fake.requests().len();
    let main = avr.zone(Zone::Main).unwrap();
    main.select_sound_mode("MOVIE").await.unwrap();
    main.select_source("GAME").await.unwrap();
    // The first read has both, and the one retry has both again.
    wait_until("the reads", || reads_after(&fake, known).len() >= 4).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
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
        ..quick_follow_up()
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
        ..quick_follow_up()
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
    let avr = connect(&fake, Some(quick_follow_up())).await;
    let known = fake.requests().len();
    avr.zone(Zone::Main).unwrap().volume_up().await.unwrap();
    wait_until("the read", || !reads_after(&fake, known).is_empty()).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(reads_after(&fake, known).len(), 1);
}

#[tokio::test]
async fn a_read_after_a_command_puts_the_values_into_the_state() {
    // The state is empty. If a read failed, the values would stay empty.
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    avr.zone(Zone::Main).unwrap().volume_up().await.unwrap();
    wait_until("the volume", || {
        avr.snapshot().zone(Zone::Main).volume.map(Volume::db) == Some(-36.5)
    })
    .await;
    assert_eq!(avr.snapshot().zone(Zone::Main).muted, Some(false));
}

#[tokio::test]
async fn commands_of_different_kinds_are_each_read() {
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
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
    let avr = connect(&fake, Some(quick_follow_up())).await;
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

#[tokio::test]
async fn a_command_goes_before_the_reads_that_wait() {
    let fake = receiver().await;
    let avr = Arc::new(
        builder(&fake, ReceiverType::AvrX)
            .http_config(HttpConfig {
                pacing: Pacing::Fixed {
                    interval: Duration::from_millis(300),
                },
                ..config()
            })
            .follow_up_read(None)
            .connect()
            .await
            .unwrap(),
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    let known = fake.requests().len();
    let reads: Vec<_> = (0..5)
        .map(|_| {
            let avr = avr.clone();
            tokio::spawn(async move { avr.update().await })
        })
        .collect();
    tokio::time::sleep(Duration::from_millis(20)).await;
    // In a task too, so that it does not run before the reads by chance.
    let command = tokio::spawn({
        let avr = avr.clone();
        async move { avr.http_send(Zone::Main, Command::VolumeUp).await }
    });
    command.await.unwrap().unwrap();
    for read in reads {
        read.await.unwrap().unwrap();
    }
    // The first read had started, the others waited for the command.
    let methods: Vec<String> = fake.requests()[known..]
        .iter()
        .map(|seen| seen.method.clone())
        .collect();
    assert_eq!(methods, ["POST", "GET", "POST", "POST", "POST", "POST"]);
}

#[tokio::test]
async fn a_read_waits_for_the_pause_after_a_command() {
    let fake = receiver().await;
    let avr = builder(&fake, ReceiverType::AvrX)
        .background_pause(Duration::from_millis(200))
        .follow_up_read(None)
        .connect()
        .await
        .unwrap();
    avr.http_send(Zone::Main, Command::VolumeUp).await.unwrap();
    let start = tokio::time::Instant::now();
    avr.update().await.unwrap();
    assert!(
        start.elapsed() >= Duration::from_millis(190),
        "{:?}",
        start.elapsed()
    );
}

#[tokio::test]
async fn a_key_that_is_held_down_is_read_before_it_is_released() {
    let fake = receiver().await;
    let avr = builder(&fake, ReceiverType::AvrX)
        .background_pause(Duration::from_millis(300))
        .follow_up_read(Some(quick_follow_up()))
        .connect()
        .await
        .unwrap();
    let known = fake.requests().len();
    // A command every 100 ms, so there is never a pause of 300 ms.
    for _ in 0..8 {
        avr.zone(Zone::Main).unwrap().volume_up().await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_ne!(reads_after(&fake, known), [] as [String; 0]);
}

/// The requests after the first `known` requests that went to `path`, with
/// their bodies.
fn bodies_to(fake: &Fake, known: usize, path: &str) -> Vec<String> {
    bodies_after(fake, known, |seen| seen.path == path)
}

#[tokio::test]
async fn a_bass_command_reads_tone_control() {
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    let known = fake.requests().len();
    avr.http_send(Zone::Main, Command::BassUp).await.unwrap();
    wait_until("the read", || !reads_after(&fake, known).is_empty()).await;
    let reads = reads_after(&fake, known);
    assert!(reads[0].contains("GetToneControl"), "{}", reads[0]);
    assert!(!reads[0].contains("GetAllZoneVolume"), "{}", reads[0]);
}

#[tokio::test]
async fn a_bass_command_in_zone_2_causes_no_read() {
    // The read of tone control has the main zone only.
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    let known = fake.requests().len();
    avr.http_send(Zone::Zone2, Command::BassUp).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(reads_after(&fake, known), [] as [String; 0]);
}

#[tokio::test]
async fn an_audyssey_command_reads_the_audyssey_settings() {
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    let known = fake.requests().len();
    avr.http_send(Zone::Main, Command::DynamicEq(true))
        .await
        .unwrap();
    let path = "/goform/AppCommand0300.xml";
    wait_until("the read", || !bodies_to(&fake, known, path).is_empty()).await;
    let reads = bodies_to(&fake, known, path);
    assert!(reads[0].contains("GetAudyssey"), "{}", reads[0]);
    assert!(!reads[0].contains("GetVideoInfo"), "{}", reads[0]);
}

#[tokio::test]
async fn a_missing_audyssey_file_is_not_asked_again() {
    // The fake receiver answers 404 for `AppCommand0300.xml`.
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    let known = fake.requests().len();
    let path = "/goform/AppCommand0300.xml";
    avr.http_send(Zone::Main, Command::DynamicEq(true))
        .await
        .unwrap();
    wait_until("the read", || !bodies_to(&fake, known, path).is_empty()).await;
    avr.http_send(Zone::Main, Command::DynamicEq(true))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(bodies_to(&fake, known, path).len(), 1);
}

/// An older receiver that refuses `GetToneControl`. It answers other
/// `AppCommand.xml` requests only if `answers_others` is true.
async fn receiver_that_refuses_tone_control(answers_others: bool) -> Fake {
    Fake::start(
        move |seen| match (seen.method.as_str(), seen.path.as_str()) {
            ("GET", "/goform/Deviceinfo.xml") => Reply::Body(AVR_X_INFO.to_vec()),
            ("GET", path) if path.starts_with(COMMAND) => Reply::Body(Vec::new()),
            ("POST", _) if is_friendly_name_request(&seen.body) => {
                Reply::Body(friendly_name_answer("X"))
            }
            ("POST", _) if contains(&seen.body, "GetToneControl") => Reply::Status(500),
            ("POST", "/goform/AppCommand.xml") if answers_others => {
                Reply::Body(update_answer_for(&seen.body))
            }
            _ => Reply::Status(404),
        },
    )
    .await
}

/// Sends two bass commands, and gives the number of tone reads after them.
/// The second command goes after the power request that follows the failed
/// tone read.
async fn tone_reads_after_two_bass_commands(fake: &Fake, avr: &Avr) -> usize {
    let known = fake.requests().len();
    let reads_of = |query: &str| {
        bodies_after(fake, known, |seen| seen.method == "POST")
            .iter()
            .filter(|body| body.contains(query))
            .count()
    };
    avr.http_send(Zone::Main, Command::BassUp).await.unwrap();
    wait_until("the power request", || {
        reads_of("GetAllZonePowerStatus") > 0
    })
    .await;
    assert_eq!(reads_of("GetToneControl"), 1);
    avr.http_send(Zone::Main, Command::BassUp).await.unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    reads_of("GetToneControl")
}

#[tokio::test]
async fn a_failed_tone_read_of_an_older_receiver_is_not_sent_again() {
    let fake = receiver_that_refuses_tone_control(true).await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    assert_eq!(tone_reads_after_two_bass_commands(&fake, &avr).await, 1);
    assert!(avr.missing_sources().tone_control);
}

#[tokio::test]
async fn a_failed_tone_read_needs_an_answered_request_to_drop_tone_control() {
    // The receiver also refuses the power request, so the failure does not
    // show that tone control is missing.
    let fake = receiver_that_refuses_tone_control(false).await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    assert_eq!(tone_reads_after_two_bass_commands(&fake, &avr).await, 2);
    assert!(!avr.missing_sources().tone_control);
}

/// A receiver that answers the HDMI output in `AppCommand0300.xml` if `info`
/// lists it.
async fn output_settings_receiver(info: &'static [u8]) -> Fake {
    Fake::start(
        move |seen| match (seen.method.as_str(), seen.path.as_str()) {
            ("GET", "/goform/Deviceinfo.xml") => Reply::Body(info.to_vec()),
            ("GET", path) if path.starts_with(COMMAND) => Reply::Body(Vec::new()),
            ("POST", _) if is_friendly_name_request(&seen.body) => {
                Reply::Body(friendly_name_answer("X"))
            }
            ("POST", "/goform/AppCommand0300.xml") => {
                Reply::Body(format!("<rx>{}</rx>", output_settings_item("2")).into_bytes())
            }
            ("POST", "/goform/AppCommand.xml") => Reply::Body(update_answer_for(&seen.body)),
            _ => Reply::Status(404),
        },
    )
    .await
}

#[tokio::test]
async fn an_hdmi_output_command_reads_the_hdmi_output() {
    let fake = output_settings_receiver(AVR_X_OUTPUT_SETTINGS_INFO).await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    let known = fake.requests().len();
    // The receiver answers HDMI 2, so the value comes from the read.
    avr.video()
        .set_hdmi_output(HdmiOutput::Hdmi1)
        .await
        .unwrap();
    wait_until("the HDMI output", || {
        avr.snapshot().device().video_settings.hdmi_output == Some(HdmiOutput::Hdmi2)
    })
    .await;
    let reads = bodies_to(&fake, known, "/goform/AppCommand0300.xml");
    assert_eq!(reads.len(), 1, "{reads:?}");
    assert!(reads[0].contains("GetOutputSettings"), "{}", reads[0]);
    assert!(!reads[0].contains("GetVideoInfo"), "{}", reads[0]);
}

#[tokio::test]
async fn an_hdmi_output_command_reads_nothing_without_the_output_settings() {
    let fake = output_settings_receiver(AVR_X_INFO).await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    let known = fake.requests().len();
    avr.video()
        .set_hdmi_output(HdmiOutput::Hdmi2)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(commands(&fake).last().map(String::as_str), Some("VSMONI2"));
    assert_eq!(
        bodies_to(&fake, known, "/goform/AppCommand0300.xml").len(),
        0
    );
}

#[tokio::test]
async fn an_audyssey_command_reads_nothing_if_the_query_is_not_listed() {
    let fake = output_settings_receiver(AUDIO_INFO_ONLY_INFO).await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    let known = fake.requests().len();
    avr.http_send(Zone::Main, Command::DynamicEq(true))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        commands(&fake).last().map(String::as_str),
        Some("PSDYNEQ%20ON")
    );
    assert_eq!(
        bodies_to(&fake, known, "/goform/AppCommand0300.xml").len(),
        0
    );
}

/// A receiver whose main zone plays the tuner.
async fn tuner_receiver() -> Fake {
    Fake::start(|seen| match (seen.method.as_str(), seen.path.as_str()) {
        ("GET", "/goform/Deviceinfo.xml") => Reply::Body(AVR_X_INFO.to_vec()),
        ("GET", TUNER_STATUS_PATH) => Reply::Body(TUNER_PAGE.to_vec()),
        ("GET", path) if path.starts_with(COMMAND) => Reply::Body(Vec::new()),
        ("POST", _) if is_friendly_name_request(&seen.body) => {
            Reply::Body(friendly_name_answer("X"))
        }
        ("POST", "/goform/AppCommand.xml") => Reply::Body(
            String::from_utf8(update_answer_for(&seen.body))
                .unwrap()
                .replace("MPLAY", "TUNER")
                .into_bytes(),
        ),
        _ => Reply::Status(404),
    })
    .await
}

#[tokio::test]
async fn a_tuner_command_reads_the_tuner_page() {
    let fake = tuner_receiver().await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    avr.update().await.unwrap();
    avr.http_send(Zone::Main, Command::Tuner(TunerCommand::FrequencyUp))
        .await
        .unwrap();
    wait_until("the frequency", || {
        avr.snapshot().device().tuner.frequency.is_some()
    })
    .await;
    let frequency = avr.snapshot().device().tuner.frequency.unwrap();
    assert_eq!(frequency.hundredths(), 8750);
}

#[tokio::test]
async fn a_tuner_command_reads_no_page_for_another_source() {
    let fake = receiver().await;
    let avr = connect(&fake, Some(quick_follow_up())).await;
    avr.update().await.unwrap();
    let known = fake.requests().len();
    avr.http_send(Zone::Main, Command::Tuner(TunerCommand::FrequencyUp))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(bodies_to(&fake, known, TUNER_STATUS_PATH).len(), 0);
}
