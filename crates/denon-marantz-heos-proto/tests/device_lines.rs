//! Parses lines that an AVC-A10H sent: answers and events.
//!
//! Addresses, ids, serial number, user names, titles and links in the data are
//! placeholders.

use denon_marantz_heos_proto::{
    Controls, Event, LineDecoder, MediaType, PlayState, Player, PlayerId, Progress, QueueId,
    QuickSelect, Response, SourceId, Status,
};

const DATA: &str = include_str!("data/a10h.jsonl");
const PID: PlayerId = PlayerId(1_234_567);

fn find(command: &str) -> impl Iterator<Item = Response<'static>> {
    DATA.lines()
        .map(|line| Response::parse(line).unwrap())
        .filter(move |response| response.command() == command)
}

#[test]
fn every_line_parses() {
    let mut events = 0;
    for line in DATA.lines() {
        let response = Response::parse(line).unwrap();
        match response.event().unwrap() {
            // Volume events are not parsed.
            Some(Event::Unknown { name, .. }) => assert_eq!(name, "player_volume_changed"),
            Some(event) => {
                assert_eq!(event.pid(), Some(PID), "{line}");
                events += 1;
            }
            None => assert_eq!(response.status(), Status::Success, "{line}"),
        }
    }
    assert!(events > 5);
}

#[test]
fn decoder_gives_the_same_lines() {
    let stream = DATA.lines().fold(String::new(), |mut stream, line| {
        stream.push_str(line);
        stream.push_str("\r\n");
        stream
    });
    let mut decoder = LineDecoder::new();
    let mut lines = Vec::new();
    for chunk in stream.as_bytes().chunks(97) {
        decoder.decode(chunk, |line| lines.push(line.to_owned()));
    }
    assert_eq!(lines, DATA.lines().collect::<Vec<_>>());
}

#[test]
fn players() {
    let response = find("player/get_players").next().unwrap();
    let players: Vec<Player<'_>> = response.payload().unwrap();
    assert_eq!(players.len(), 1);
    let player = players[0].clone().into_owned();
    assert_eq!(player.pid, PID);
    assert_eq!(player.name, "Denon AVC-A10H");
    assert_eq!(player.version.as_deref(), Some("3.139.173"));
    assert_eq!(player.ip_addr(), Some([192, 168, 1, 20].into()));
    assert_eq!(player.lineout, Some(0));
    assert_eq!(player.gid, None);

    let info = find("player/get_player_info").next().unwrap();
    assert_eq!(info.payload::<Player<'_>>().unwrap(), players[0]);
}

#[test]
fn now_playing_station_and_song() {
    let all: Vec<_> = find("player/get_now_playing_media")
        .map(|response| response.now_playing().unwrap().unwrap().into_owned())
        .collect();

    let song = all
        .iter()
        .find(|now| now.media_type == MediaType::Song)
        .unwrap();
    assert_eq!(song.song, "Track 2.wav");
    assert_eq!(song.station, None);
    assert_eq!(song.qid, Some(QueueId(1)));
    assert_eq!(song.sid, Some(SourceId::LOCAL_MUSIC));

    let tunein = all
        .iter()
        .find(|now| now.sid == Some(SourceId::TUNEIN))
        .unwrap();
    assert_eq!(tunein.media_type, MediaType::Station);
    assert_eq!(tunein.station.as_deref(), Some("Example Radio"));
    assert_eq!(tunein.artist, "Artist A");
    assert_eq!(
        Controls::for_media(tunein.sid, tunein.media_type),
        Controls::for_media(Some(SourceId::IHEARTRADIO), MediaType::Station)
    );

    // AirPlay: a station of the local music source with a numeric media id.
    let airplay = all
        .iter()
        .find(|now| now.sid == Some(SourceId::LOCAL_MUSIC) && now.media_type == MediaType::Station)
        .unwrap();
    assert_eq!(airplay.song, airplay.station.as_deref().unwrap());
    assert!(
        airplay
            .mid
            .as_deref()
            .unwrap()
            .bytes()
            .all(|b| b.is_ascii_digit())
    );
    assert_eq!(
        Controls::for_media(airplay.sid, airplay.media_type),
        Controls::ALL
    );
}

#[test]
fn quick_selects() {
    let response = find("player/get_quickselects").next().unwrap();
    let slots: Vec<QuickSelect<'_>> = response.payload().unwrap();
    assert_eq!(slots.len(), 4);
    assert_eq!(slots[3].id, 4);
    assert_eq!(slots[3].name, "Quick Select 4");
}

#[test]
fn play_state_and_progress() {
    assert!(find("player/get_play_state").any(|response| {
        PlayState::from_name(&response.message().get("state").unwrap()) == Some(PlayState::Play)
    }));

    let progress = find("event/player_now_playing_progress").next().unwrap();
    assert!(matches!(
        progress.event().unwrap(),
        Some(Event::NowPlayingProgress {
            progress: Progress { duration_ms: 0, .. },
            ..
        })
    ));
}

#[test]
fn playback_error_text() {
    let response = find("event/player_playback_error").next().unwrap();
    let Some(Event::PlaybackError { error, .. }) = response.event().unwrap() else {
        panic!("not a playback error");
    };
    assert_eq!(error, "Playback error. Could not decode the audio stream");
}
