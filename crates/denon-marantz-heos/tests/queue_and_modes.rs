//! Play mode, queue, HEOS Favorites, quick selects and the play commands
//! against a fake HEOS device.

mod common;

use std::time::Duration;

use common::{
    Fake, PID, Reply, answer, config, connect, echo, event, from_data, name, next_change,
    under_process,
};
use denon_marantz_heos::{
    Change, Error, Heos, HeosConfig, MediaType, PlayMode, PlayerId, QueueId, RepeatMode,
};

/// Answers from the data. Commands without an answer there get [`echo`].
fn device(line: &str) -> Reply {
    match name(line) {
        "player/play_queue"
        | "player/remove_from_queue"
        | "player/clear_queue"
        | "player/play_quickselect"
        | "browse/play_preset"
        | "browse/play_stream"
        | "browse/play_input" => Reply::Lines(vec![echo(line)]),
        _ => from_data(line),
    }
}

#[tokio::test]
async fn play_mode_events_and_answers_change_the_state() {
    let fake = Fake::start(device).await;
    let heos = connect(&fake).await;
    let mut changes = heos.changes();
    let mode = |repeat, shuffle| Some(PlayMode { repeat, shuffle });
    assert_eq!(
        heos.player(PID).unwrap().play_mode(),
        mode(RepeatMode::Off, false)
    );

    fake.push(event("repeat_mode_changed", "pid=1234567&repeat=on_all"));
    assert_eq!(next_change(&mut changes).await, Change::PlayMode(PID));
    fake.push(event("shuffle_mode_changed", "pid=1234567&shuffle=on"));
    assert_eq!(next_change(&mut changes).await, Change::PlayMode(PID));
    assert_eq!(
        heos.player(PID).unwrap().play_mode(),
        mode(RepeatMode::OnAll, true)
    );

    // The answer has the new modes, so the state changes without an event.
    heos.set_play_mode(
        PID,
        PlayMode {
            repeat: RepeatMode::OnOne,
            shuffle: true,
        },
    )
    .await
    .unwrap();
    assert_eq!(
        heos.player(PID).unwrap().play_mode(),
        mode(RepeatMode::OnOne, true)
    );
    assert_eq!(next_change(&mut changes).await, Change::PlayMode(PID));

    fake.push(event("player_queue_changed", "pid=1234567"));
    assert_eq!(next_change(&mut changes).await, Change::Queue(PID));
}

#[tokio::test]
async fn a_reconnect_tells_that_the_queue_may_have_changed() {
    let fake = Fake::start(device).await;
    let heos = connect(&fake).await;
    let mut changes = heos.changes();
    fake.close_all();
    loop {
        if next_change(&mut changes).await == Change::Queue(PID) {
            break;
        }
    }
    assert_eq!(fake.count("player/get_players"), 2);
}

#[tokio::test]
async fn lists_are_read_in_pages() {
    let fake = Fake::start(device).await;
    let heos = connect(&fake).await;

    let queue = heos.queue(PID, Some((0, 9))).await.unwrap();
    assert_eq!(queue.count, Some(3));
    assert_eq!(queue.items.len(), 3);
    assert_eq!(queue.items[0].qid, QueueId(1));
    assert_eq!(queue.items[0].song, "Track 2.wav");

    // The device sends "command under process" first.
    let favorites = heos.favorites(None).await.unwrap();
    assert_eq!(favorites.count, Some(2));
    let names: Vec<_> = favorites.items.iter().map(|item| &*item.name).collect();
    assert_eq!(names, ["Radio A", "Radio B"]);
    assert_eq!(favorites.items[0].media_type, MediaType::Station);

    let slots = heos.quick_selects(PID).await.unwrap();
    assert_eq!(slots.len(), 4);
    assert_eq!(slots[0].name, "Quick Select 1");

    let seen = fake.seen();
    assert!(seen.contains(&"heos://player/get_queue?pid=1234567&range=0,9".to_owned()));
    assert!(seen.contains(&"heos://browse/browse?sid=1028".to_owned()));
}

#[tokio::test]
async fn a_list_without_payload_is_empty() {
    let fake = Fake::start(|line| match name(line) {
        "player/get_queue" => Reply::Lines(vec![echo(line)]),
        "player/get_quickselects" => Reply::Lines(vec![
            r#"{"heos": {"command": "player/get_quickselects", "result": "success", "message": "pid=1234567"}, "payload": {"id": 1}}"#.to_owned(),
        ]),
        _ => from_data(line),
    })
    .await;
    let heos = connect(&fake).await;
    let queue = heos.queue(PID, None).await.unwrap();
    assert_eq!(queue.items.len(), 0);
    assert_eq!(queue.count, None);
    // A payload that is not a list is an error.
    let error = heos.quick_selects(PID).await.unwrap_err();
    assert!(
        matches!(
            error,
            Error::Protocol {
                command: "player/get_quickselects",
                ..
            }
        ),
        "{error:?}"
    );
}

#[tokio::test]
async fn play_commands_send_their_values() {
    let fake = Fake::start(device).await;
    let heos = connect(&fake).await;
    heos.play_queue_item(PID, QueueId(2)).await.unwrap();
    heos.remove_from_queue(PID, &[QueueId(2), QueueId(3)])
        .await
        .unwrap();
    heos.clear_queue(PID).await.unwrap();
    heos.play_preset(PID, 1).await.unwrap();
    heos.play_quick_select(PID, 6).await.unwrap();
    heos.play_stream(PID, "http://host/a b.mp3?x=1&y=2")
        .await
        .unwrap();
    heos.play_input(PID, Some(PlayerId(7)), "inputs/aux_in_1")
        .await
        .unwrap();
    let seen = fake.seen();
    let start = seen
        .iter()
        .position(|line| line.contains("play_queue"))
        .unwrap();
    assert_eq!(
        seen[start..],
        [
            "heos://player/play_queue?pid=1234567&qid=2",
            "heos://player/remove_from_queue?pid=1234567&qid=2,3",
            "heos://player/clear_queue?pid=1234567",
            "heos://browse/play_preset?pid=1234567&preset=1",
            "heos://player/play_quickselect?pid=1234567&id=6",
            "heos://browse/play_stream?pid=1234567&url=http://host/a b.mp3?x=1&y=2",
            "heos://browse/play_input?pid=1234567&spid=7&input=inputs/aux_in_1",
        ]
    );
    // Values out of range are not sent.
    for error in [
        heos.remove_from_queue(PID, &[]).await,
        heos.play_preset(PID, 0).await,
        heos.play_quick_select(PID, 7).await,
    ] {
        assert!(error.unwrap_err().is_invalid_input());
    }
    assert_eq!(fake.seen().len(), seen.len());
}

#[tokio::test]
async fn under_process_allows_more_time() {
    // A line every 600 ms: the answer comes after 1800 ms. The command timeout
    // is 900 ms, so both "under process" lines must extend the deadline. The
    // slow command limit is 3000 ms. Large gaps keep the test stable on slow
    // machines.
    let fake = Fake::start(|line| match name(line) {
        "browse/browse" => Reply::Paced(
            Duration::from_millis(600),
            vec![
                under_process("browse/browse"),
                under_process("browse/browse"),
                answer("browse/browse"),
            ],
        ),
        _ => from_data(line),
    })
    .await;
    let heos = Heos::connect_with(
        "127.0.0.1",
        HeosConfig {
            command_timeout: Duration::from_millis(900),
            slow_command_limit: Duration::from_millis(3000),
            ..config(fake.port)
        },
    )
    .await
    .unwrap();
    assert_eq!(heos.favorites(None).await.unwrap().items.len(), 2);
}

#[tokio::test]
async fn under_process_has_a_limit() {
    let fake = Fake::start(|line| match name(line) {
        "browse/browse" => Reply::Paced(
            Duration::from_millis(300),
            vec![under_process("browse/browse"); 6],
        ),
        _ => from_data(line),
    })
    .await;
    let heos = connect(&fake).await;
    let error = heos.favorites(None).await.unwrap_err();
    let Error::Timeout { request, limit } = error else {
        panic!("not a timeout: {error:?}");
    };
    assert_eq!(request, "browse/browse");
    assert_eq!(limit, Duration::from_millis(1200));
}
