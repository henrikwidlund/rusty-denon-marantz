//! The client against a fake HEOS device.

mod common;

use std::time::{Duration, Instant};

use common::{
    Fake, IP, LOOPBACK_V6, LogBuffer, PID, REGISTER, Reply, answer, answer_lines, config, connect,
    counter, event, failure, from_data, name, next_change, now_playing, wait_for_registers,
    wait_until,
};
use denon_marantz_heos::proto::ErrorCode;
use denon_marantz_heos::{
    Change, Command, Error, Event, Heos, HeosConfig, HeosStatus, PlayState, PlayerId, PlayerState,
    Progress, RepeatMode,
};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::{RecvError, TryRecvError};

#[tokio::test]
async fn connect_reads_the_players_and_their_state() {
    let fake = Fake::start(from_data).await;
    let heos = connect(&fake).await;
    assert_eq!(heos.status(), HeosStatus::Connected);
    let players = heos.players();
    assert_eq!(players.len(), 1);
    let state = &players[0];
    assert_eq!(state.pid(), PID);
    assert_eq!(state.play_state, Some(PlayState::Stop));
    assert_eq!(
        state.now_playing.as_deref().map(|media| &*media.song),
        Some("Track 2.wav")
    );
    // A stopped player hides its last item.
    assert_eq!(state.current_media(), None);
    assert_eq!(heos.player_by_ip(IP.parse().unwrap()), Some(PID));
    assert_eq!(heos.player_by_ip("192.168.1.21".parse().unwrap()), None);
    let mapped = format!("::ffff:{IP}");
    assert_eq!(heos.player_by_ip(mapped.parse().unwrap()), Some(PID));
    assert_eq!(heos.player(PlayerId(1)), None);
    assert_eq!(heos.with_player(PID, PlayerState::pid), Some(PID));
    assert_eq!(heos.with_player(PlayerId(1), PlayerState::pid), None);
    let seen = fake.seen();
    assert!(seen.contains(&format!("heos://{REGISTER}?enable=on")));
    let commands: Vec<_> = seen
        .iter()
        .filter(|line| !line.contains("register"))
        .map(String::as_str)
        .collect();
    assert_eq!(
        commands,
        [
            "heos://player/get_players",
            "heos://player/get_play_state?pid=1234567",
            "heos://player/get_now_playing_media?pid=1234567",
            "heos://player/get_play_mode?pid=1234567",
        ]
    );
    assert_eq!(state.repeat, Some(RepeatMode::Off));
    assert_eq!(state.shuffle, Some(false));
    assert_eq!(fake.connections(), 2);
    heos.close().await;
}

#[tokio::test]
async fn events_change_the_state() {
    let fake = Fake::start(from_data).await;
    let heos = connect(&fake).await;
    let mut changes = heos.changes();
    let mut events = heos.events();
    fake.push(event("player_state_changed", "pid=1234567&state=play"));
    assert_eq!(next_change(&mut changes).await, Change::PlayState(PID));
    assert_eq!(heos.player(PID).unwrap().play_state, Some(PlayState::Play));
    fake.push(event(
        "player_now_playing_progress",
        "pid=1234567&cur_pos=3000&duration=0",
    ));
    assert_eq!(next_change(&mut changes).await, Change::Progress(PID));
    fake.push(event(
        "player_playback_error",
        "pid=1234567&error=Bad %26 worse",
    ));
    assert_eq!(
        next_change(&mut changes).await,
        Change::PlaybackError {
            pid: PID,
            error: "Bad & worse".into()
        }
    );
    // The same state again is no change. Volume events have no change.
    fake.push(event("player_state_changed", "pid=1234567&state=play"));
    fake.push(event(
        "player_volume_changed",
        "pid=1234567&level=1&mute=off",
    ));
    fake.push(event("player_state_changed", "pid=1234567&state=pause"));
    assert_eq!(next_change(&mut changes).await, Change::PlayState(PID));
    let events: Vec<_> = std::iter::from_fn(|| events.try_recv().ok()).collect();
    let state = |state| Event::PlayerStateChanged {
        pid: PID,
        state: Some(state),
    };
    assert_eq!(
        events,
        [
            state(PlayState::Play),
            Event::NowPlayingProgress {
                pid: PID,
                progress: Progress {
                    position_ms: 3000,
                    duration_ms: 0
                }
            },
            Event::PlaybackError {
                pid: PID,
                error: "Bad & worse".into()
            },
            state(PlayState::Play),
            Event::Unknown {
                name: "player_volume_changed".into(),
                message: "pid=1234567&level=1&mute=off".into()
            },
            state(PlayState::Pause),
        ]
    );
}

#[tokio::test]
async fn unknown_play_state_is_none_and_clears_the_progress() {
    let fake = Fake::start(|line| {
        if name(line) != "player/get_play_state" {
            return from_data(line);
        }
        let answer = answer("player/get_play_state").replace("state=stop", "state=unknown");
        Reply::Lines(vec![answer])
    })
    .await;
    let heos = connect(&fake).await;
    assert_eq!(heos.player(PID).unwrap().play_state, None);
    let mut changes = heos.changes();
    fake.push(event("player_state_changed", "pid=1234567&state=play"));
    fake.push(event(
        "player_now_playing_progress",
        "pid=1234567&cur_pos=3000&duration=0",
    ));
    fake.push(event("player_state_changed", "pid=1234567&state=unknown"));
    for expected in [
        Change::PlayState(PID),
        Change::Progress(PID),
        Change::PlayState(PID),
        Change::Progress(PID),
    ] {
        assert_eq!(next_change(&mut changes).await, expected);
    }
    let state = heos.player(PID).unwrap();
    assert_eq!(state.play_state, None);
    assert_eq!(state.progress, None);
    heos.close().await;
}

#[tokio::test]
async fn a_burst_of_now_playing_events_gives_one_read() {
    let reads = counter();
    let fake = Fake::start(move |line| {
        if name(line) != "player/get_now_playing_media" {
            return from_data(line);
        }
        // The first read gives the first item, later reads another one.
        let song = if reads() == 0 {
            "Track 2.wav"
        } else {
            "Song A"
        };
        Reply::Lines(vec![now_playing(song)])
    })
    .await;
    let heos = connect(&fake).await;
    let mut changes = heos.changes();
    // One write, so that a slow test machine does not spread the burst.
    let burst = vec![event("player_now_playing_changed", "pid=1234567"); 5];
    fake.push(burst.join("\r\n"));
    assert_eq!(next_change(&mut changes).await, Change::NowPlaying(PID));
    let state = heos.player(PID).unwrap();
    assert_eq!(state.now_playing.unwrap().song, "Song A");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(fake.count("player/get_now_playing_media"), 2);
}

#[tokio::test]
async fn players_changed_reads_the_players_again() {
    let fake = Fake::start(from_data).await;
    let heos = connect(&fake).await;
    fake.push(event("players_changed", ""));
    wait_until(|| fake.count("player/get_players") == 2).await;
    wait_until(|| fake.count("player/get_now_playing_media") == 2).await;
    heos.refresh().await.unwrap();
    assert_eq!(fake.count("player/get_players"), 3);
    assert_eq!(fake.count("player/get_play_state"), 3);
}

#[tokio::test]
async fn a_player_with_a_mapped_ipv6_address_is_found_by_its_ipv4_address() {
    let fake = Fake::start(|line| match from_data(line) {
        Reply::Lines(lines) => Reply::Lines(
            lines
                .into_iter()
                .map(|line| line.replace(IP, &format!("::ffff:{IP}")))
                .collect(),
        ),
        reply => reply,
    })
    .await;
    let heos = connect(&fake).await;
    assert_eq!(heos.player_by_ip(IP.parse().unwrap()), Some(PID));
}

#[tokio::test]
async fn commands_give_the_answer_or_the_failure() {
    let fake = Fake::start(|line| match name(line) {
        "player/play_previous" => Reply::Lines(vec![failure(
            "player/play_previous",
            "eid=2&text=ID Not Valid&pid=1234567",
        )]),
        // The device works on the command first. An answer to another
        // command does not count.
        "player/play_next" => Reply::Lines(vec![
            r#"{"heos": {"command": "player/play_next", "result": "success", "message": "command under process&pid=1234567"}}"#.to_owned(),
            answer("player/get_play_state"),
            answer("player/play_next"),
        ]),
        _ => from_data(line),
    })
    .await;
    let heos = connect(&fake).await;
    heos.set_play_state(PID, PlayState::Pause).await.unwrap();
    heos.play_next(PID).await.unwrap();
    let error = heos.play_previous(PID).await.unwrap_err();
    let Error::Failed { command, failure } = error else {
        panic!("not a failure: {error:?}");
    };
    assert_eq!(command, "player/play_previous");
    assert_eq!(failure.code, Some(ErrorCode::InvalidId));
    // The answer to a read changes the state.
    let answer = heos.send(Command::GetPlayState { pid: PID }).await.unwrap();
    assert_eq!(answer.response().command(), "player/get_play_state");
    assert_eq!(heos.status(), HeosStatus::Connected);
}

#[tokio::test]
async fn an_invalid_command_is_not_sent() {
    let fake = Fake::start(from_data).await;
    let heos = connect(&fake).await;
    let before = fake.seen().len();
    let error = heos
        .send(Command::PlayStream {
            pid: PID,
            url: "http://a\r\nheos://system/reboot",
        })
        .await
        .unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    heos.play_next(PID).await.unwrap();
    assert_eq!(fake.seen().len(), before + 1);
}

#[tokio::test]
async fn a_command_whose_caller_stopped_waiting_is_not_sent() {
    let fake = Fake::start(|line| match name(line) {
        "player/play_next" => {
            Reply::Paced(Duration::from_millis(300), vec![answer("player/play_next")])
        }
        _ => from_data(line),
    })
    .await;
    let heos = connect(&fake).await;
    let (slow, given_up) = tokio::join!(heos.play_next(PID), async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        tokio::time::timeout(Duration::from_millis(50), heos.play_previous(PID)).await
    });
    slow.unwrap();
    assert!(given_up.is_err());
    heos.set_play_state(PID, PlayState::Pause).await.unwrap();
    assert_eq!(fake.count("player/play_previous"), 0);
}

#[tokio::test]
async fn a_command_sent_when_the_connection_closes_may_be_done() {
    let fake = Fake::start(|line| match name(line) {
        "player/play_previous" => Reply::Hang,
        _ => from_data(line),
    })
    .await;
    let heos = connect(&fake).await;
    let (error, ()) = tokio::join!(heos.play_previous(PID), async {
        wait_until(|| fake.count("player/play_previous") == 1).await;
        fake.close_all();
    });
    let error = error.unwrap_err();
    assert!(
        matches!(
            error,
            Error::Disconnected {
                command: "player/play_previous"
            }
        ),
        "{error:?}"
    );
    assert!(!error.is_not_sent() && error.is_network());
}

#[tokio::test]
async fn a_command_without_an_answer_times_out_and_connects_again() {
    let fake = Fake::start(|line| match name(line) {
        "player/play_previous" => Reply::Hang,
        _ => from_data(line),
    })
    .await;
    let heos = connect(&fake).await;
    let mut status = heos.watch_status();
    // A command that waits behind it was not sent.
    let (error, waiting) = tokio::join!(heos.play_previous(PID), async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        heos.play_next(PID).await
    });
    let error = error.unwrap_err();
    assert!(
        matches!(error, Error::Timeout { sent: true, .. }),
        "{error:?}"
    );
    assert!(!error.is_not_sent());
    assert!(waiting.unwrap_err().is_not_connected());
    status
        .wait_for(|status| *status == HeosStatus::Connected)
        .await
        .unwrap();
    wait_until(|| fake.count("player/get_now_playing_media") == 2).await;
    assert_eq!(fake.connections(), 4);
    wait_for_registers(&fake, 2).await;
    heos.play_next(PID).await.unwrap();
}

#[tokio::test]
async fn a_closed_connection_is_opened_again() {
    let fake = Fake::start(from_data).await;
    let heos = connect(&fake).await;
    let mut status = heos.watch_status();
    fake.close_all();
    status
        .wait_for(|status| *status == HeosStatus::Disconnected)
        .await
        .unwrap();
    assert!(heos.play_next(PID).await.unwrap_err().is_not_connected());
    status
        .wait_for(|status| *status == HeosStatus::Connected)
        .await
        .unwrap();
    wait_until(|| fake.count("player/get_now_playing_media") == 2).await;
    // Events come again.
    let mut changes = heos.changes();
    fake.push(event("player_state_changed", "pid=1234567&state=play"));
    assert_eq!(next_change(&mut changes).await, Change::PlayState(PID));
}

#[tokio::test]
async fn a_refused_register_fails_the_connect() {
    // The players come late, so the refused register ends the first read.
    let fake = Fake::start(|line| match name(line) {
        "player/get_players" => Reply::Paced(Duration::from_millis(300), answer_lines(name(line))),
        _ => from_data(line),
    })
    .await;
    fake.refuse_registers(1);
    let error = Heos::connect_with("127.0.0.1", config(fake.port))
        .await
        .err()
        .unwrap();
    assert_eq!(
        error.to_string(),
        "system/register_for_change_events failed: error 12: System error (syserrno -9)"
    );
}

#[tokio::test]
async fn a_refused_register_opens_the_connections_again() {
    let fake = Fake::start(from_data).await;
    let heos = connect(&fake).await;
    wait_for_registers(&fake, 1).await;
    let mut status = heos.watch_status();
    fake.refuse_registers(1);
    fake.close_all();
    // Without events the state would go stale, so both connections open a
    // third time.
    wait_for_registers(&fake, 3).await;
    status
        .wait_for(|status| *status == HeosStatus::Connected)
        .await
        .unwrap();
    assert_eq!(fake.connections(), 6);
    let mut changes = heos.changes();
    fake.push(event("player_state_changed", "pid=1234567&state=play"));
    // The state is read again after the reconnect, which gives other changes.
    while next_change(&mut changes).await != Change::PlayState(PID) {}
}

#[tokio::test]
async fn a_sent_command_is_not_failed_by_a_refused_register() {
    let fake = Fake::start(|line| match name(line) {
        "player/play_next" => Reply::Hang,
        _ => from_data(line),
    })
    .await;
    let heos = Heos::connect_with(
        "127.0.0.1",
        HeosConfig {
            command_timeout: Duration::from_secs(2),
            ..config(fake.port)
        },
    )
    .await
    .unwrap();
    wait_for_registers(&fake, 1).await;
    let mut status = heos.watch_status();
    // The refusal comes late, so that the command is written first.
    fake.refuse_registers(1);
    fake.delay_refusals(Duration::from_millis(300));
    fake.close_all();
    status
        .wait_for(|status| *status == HeosStatus::Disconnected)
        .await
        .unwrap();
    status
        .wait_for(|status| *status == HeosStatus::Connected)
        .await
        .unwrap();
    let error = heos.play_next(PID).await.unwrap_err();
    // The device may have done the command, so it is not `Failed`.
    assert!(
        matches!(
            error,
            Error::Disconnected {
                command: "player/play_next"
            }
        ),
        "{error:?}"
    );
    assert!(!error.is_not_sent());
    assert_eq!(fake.count("player/play_next"), 1);
}

#[tokio::test]
async fn the_wait_doubles_with_each_refused_register_in_a_row() {
    let fake = Fake::start(from_data).await;
    let heos = Heos::connect_with(
        "127.0.0.1",
        HeosConfig {
            reconnect_min: Duration::from_millis(100),
            reconnect_max: Duration::from_secs(2),
            ..config(fake.port)
        },
    )
    .await
    .unwrap();
    wait_for_registers(&fake, 1).await;
    let mut status = heos.watch_status();
    let mut reconnect = async |refusals: usize| {
        let registers = fake.count(REGISTER) + refusals + 1;
        fake.refuse_registers(refusals);
        let start = Instant::now();
        fake.close_all();
        wait_for_registers(&fake, registers).await;
        let waited = start.elapsed();
        status
            .wait_for(|status| *status == HeosStatus::Connected)
            .await
            .unwrap();
        waited
    };
    // Waits of 100, 100, 200 and 400 ms. Without doubling: 400 ms.
    let waited = reconnect(3).await;
    assert!(waited >= Duration::from_millis(700), "{waited:?}");
    // The register worked, so the wait starts again at 100 ms: 100 and 100 ms.
    let waited = reconnect(1).await;
    assert!(waited < Duration::from_millis(700), "{waited:?}");
}

#[tokio::test]
async fn idle_connections_send_heartbeats() {
    let fake = Fake::start(from_data).await;
    let heos = Heos::connect_with(
        "127.0.0.1",
        HeosConfig {
            keep_alive: Duration::from_millis(100),
            ..config(fake.port)
        },
    )
    .await
    .unwrap();
    // One on each connection.
    wait_until(|| fake.count("system/heart_beat") >= 2).await;
    assert_eq!(heos.status(), HeosStatus::Connected);
}

#[tokio::test]
async fn a_silent_device_counts_as_lost() {
    let fake = Fake::start(|line| match name(line) {
        "system/heart_beat" => Reply::Hang,
        _ => from_data(line),
    })
    .await;
    let heos = Heos::connect_with(
        "127.0.0.1",
        HeosConfig {
            keep_alive: Duration::from_millis(100),
            command_timeout: Duration::from_secs(5),
            ..config(fake.port)
        },
    )
    .await
    .unwrap();
    let mut status = heos.watch_status();
    status
        .wait_for(|status| *status == HeosStatus::Disconnected)
        .await
        .unwrap();
    wait_until(|| fake.connections() >= 4).await;
}

#[tokio::test]
async fn shutdown_closes_the_connections_for_all_references() {
    let fake = Fake::start(from_data).await;
    let heos = std::sync::Arc::new(connect(&fake).await);
    let other = std::sync::Arc::clone(&heos);
    heos.shutdown();
    // Refused at once, before the task has ended.
    assert!(other.play_next(PID).await.unwrap_err().is_not_connected());
    assert_eq!(fake.count("player/play_next"), 0);
    let mut status = other.watch_status();
    let closed = status.wait_for(|now| *now == HeosStatus::Disconnected);
    tokio::time::timeout(Duration::from_secs(2), closed)
        .await
        .unwrap()
        .unwrap();
    assert!(other.play_next(PID).await.unwrap_err().is_not_connected());
    // It does not connect again.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(fake.connections(), 2);
}

/// Waits until the watch closes. Values that come before are skipped.
async fn until_watch_closed<T: Send + Sync>(receiver: &mut tokio::sync::watch::Receiver<T>) {
    let closed = async { while receiver.changed().await.is_ok() {} };
    tokio::time::timeout(Duration::from_secs(2), closed)
        .await
        .expect("not closed");
}

/// Waits until the receiver closes. Values sent before are skipped.
async fn until_closed<T: Clone>(receiver: &mut broadcast::Receiver<T>) {
    let closed = async {
        loop {
            match receiver.recv().await {
                Err(RecvError::Closed) => break,
                Ok(_) | Err(RecvError::Lagged(_)) => {}
            }
        }
    };
    tokio::time::timeout(Duration::from_secs(2), closed)
        .await
        .expect("not closed");
}

#[tokio::test]
async fn receivers_close_when_the_task_ends() {
    let fake = Fake::start(from_data).await;
    let heos = connect(&fake).await;
    let mut changes = heos.changes();
    let mut events = heos.events();
    let mut players = heos.watch_players();
    assert!(!heos.is_shut_down());
    heos.shutdown();
    assert!(heos.is_shut_down());
    until_closed(&mut changes).await;
    until_closed(&mut events).await;
    until_watch_closed(&mut players).await;
    // New receivers are closed at once.
    assert_eq!(heos.changes().try_recv(), Err(TryRecvError::Closed));
    assert!(matches!(
        heos.events().try_recv(),
        Err(TryRecvError::Closed)
    ));
    assert!(heos.watch_players().has_changed().is_err());
    // The last state can still be read.
    assert_eq!(heos.status(), HeosStatus::Disconnected);
    assert_eq!(heos.players().len(), 1);
}

#[tokio::test]
async fn receivers_close_after_close() {
    let fake = Fake::start(from_data).await;
    let heos = connect(&fake).await;
    let mut changes = heos.changes();
    let mut events = heos.events();
    let mut players = heos.watch_players();
    heos.close().await;
    until_closed(&mut changes).await;
    until_closed(&mut events).await;
    until_watch_closed(&mut players).await;
}

#[tokio::test]
async fn an_ipv6_host_works_with_and_without_brackets() {
    let fake = Fake::start_on(LOOPBACK_V6, from_data).await;
    for host in ["[::1]", "::1"] {
        let heos = Heos::connect_with(host, config(fake.port)).await.unwrap();
        assert_eq!(heos.status(), HeosStatus::Connected);
    }
}

#[tokio::test]
async fn connect_fails_without_a_device() {
    let error = Heos::connect_with("127.0.0.1", config(common::CLOSED_PORT))
        .await
        .unwrap_err();
    // Windows reports a refused connection after about two seconds, so the
    // short timeout of the test can come first.
    assert!(
        matches!(error, Error::Network { .. })
            || (cfg!(windows) && matches!(error, Error::Timeout { .. })),
        "{error:?}"
    );
}

#[tokio::test]
async fn connect_fails_if_the_players_are_not_read() {
    let fake = Fake::start(|line| match name(line) {
        "player/get_players" => Reply::Hang,
        _ => from_data(line),
    })
    .await;
    let error = Heos::connect_with("127.0.0.1", config(fake.port))
        .await
        .unwrap_err();
    assert!(matches!(error, Error::Timeout { .. }), "{error:?}");
}

#[tokio::test]
async fn connect_fails_if_the_device_refuses_the_players() {
    let fake = Fake::start(|line| match name(line) {
        "player/get_players" => Reply::Lines(vec![failure(
            "player/get_players",
            "eid=11&text=System Internal Error",
        )]),
        _ => from_data(line),
    })
    .await;
    let error = Heos::connect_with("127.0.0.1", config(fake.port))
        .await
        .unwrap_err();
    let Error::Failed { command, failure } = error else {
        panic!("not a failure: {error:?}");
    };
    assert_eq!(command, "player/get_players");
    assert_eq!(failure.code, Some(ErrorCode::InternalError));
}

#[tokio::test]
async fn refresh_fails_if_the_players_are_not_valid() {
    let reads = counter();
    let fake = Fake::start(move |line| {
        // The second read of the players gives a player without an id.
        if name(line) == "player/get_players" && reads() == 1 {
            return Reply::Lines(vec![
                r#"{"heos": {"command": "player/get_players", "result": "success", "message": ""}, "payload": [{"name": "A"}]}"#.to_owned(),
            ]);
        }
        from_data(line)
    })
    .await;
    let heos = connect(&fake).await;
    let mut changes = heos.changes();
    let error = heos.refresh().await.unwrap_err();
    assert_eq!(next_change(&mut changes).await, Change::PlayersNotRead);
    assert!(
        matches!(
            error,
            Error::Protocol {
                command: "player/get_players",
                ..
            }
        ),
        "{error:?}"
    );
    // The players stay as they were.
    assert_eq!(heos.players().len(), 1);
}

#[tokio::test]
async fn an_item_without_text_is_no_media() {
    let reads = counter();
    let fake = Fake::start(move |line| {
        if name(line) != "player/get_now_playing_media" {
            return from_data(line);
        }
        // The first read gives a track, the second the AirPlay item without
        // text.
        let song = if reads() == 0 { "Track 2.wav" } else { "" };
        Reply::Lines(vec![now_playing(song)])
    })
    .await;
    let heos = connect(&fake).await;
    assert!(heos.player(PID).unwrap().now_playing.is_some());
    let mut changes = heos.changes();
    fake.push(event("player_now_playing_changed", "pid=1234567"));
    assert_eq!(next_change(&mut changes).await, Change::NowPlaying(PID));
    assert_eq!(heos.player(PID).unwrap().now_playing, None);
}

#[tokio::test]
async fn a_negative_position_or_length_is_skipped() {
    let fake = Fake::start(from_data).await;
    let heos = connect(&fake).await;
    let mut changes = heos.changes();
    let mut events = heos.events();
    let progress_of = |position: u64, duration: u64| {
        event(
            "player_now_playing_progress",
            &format!("pid=1234567&cur_pos={position}&duration={duration}"),
        )
    };
    let progress = |position| progress_of(position, 413_000);
    let position = || {
        heos.player(PID)
            .unwrap()
            .progress
            .map(|progress| progress.position_ms)
    };
    fake.push(progress(2_000));
    assert_eq!(next_change(&mut changes).await, Change::Progress(PID));
    // -30000 as the device sends it, as the position and as the length,
    // then a valid position.
    fake.push(progress(4_294_937_296));
    fake.push(progress_of(3_000, 4_294_937_296));
    fake.push(progress(7_000));
    wait_until(|| position() == Some(7_000)).await;
    // Only the valid position changed the state.
    let received: Vec<_> = std::iter::from_fn(|| changes.try_recv().ok()).collect();
    assert_eq!(received, [Change::Progress(PID)]);
    // Subscribers of the events still get it as the device sent it.
    let positions: Vec<_> = std::iter::from_fn(|| events.try_recv().ok())
        .filter_map(|event| match event {
            Event::NowPlayingProgress { progress, .. } => Some(progress.position_ms),
            _ => None,
        })
        .collect();
    assert_eq!(positions, [2_000, 4_294_937_296, 3_000, 7_000]);
}

#[tokio::test]
async fn the_worker_logs_in_the_heos_span() {
    let (logs, _guard) = LogBuffer::capture("denon_marantz_heos=debug");
    let fake = Fake::start(from_data).await;
    let _heos = connect(&fake).await;
    wait_until(|| logs.text().contains("HEOS connected")).await;
    let span = logs.span_of("HEOS connected", 0, "heos");
    assert!(span.contains("host=127.0.0.1"), "{span}");
    logs.assert_all_in(0, &[&span]);
}
