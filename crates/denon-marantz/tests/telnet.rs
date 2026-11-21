//! The telnet connection against a fake telnet port.

#![cfg(feature = "telnet")]

mod common;

use std::time::Duration;

use common::{
    AVR_X_DIRAC_INFO, AVR_X_INFO, AVR_X_OUTPUT_SETTINGS_INFO, AVR_X_SETTINGS_INFO, FakeTelnet,
    LOOPBACK_V6, Reply, appcommand0300_answer, avr_x_receiver, avr_x_receiver_on,
    avr_x_receiver_with_info, avr_x_receiver_with_pages, builder, contains, output_settings_item,
    quick_follow_up, receiver_with_audyssey, receiver_with_info_and_signal_answers, wait_until,
};
use denon_marantz::{
    Ask, Avr, Blocker, Command, Error, Feature, PageStatus, Query, QueryGroup, SettingStatus,
    SkipReason, TelnetConfig, TelnetEvent, TelnetStatus, ToneBlocker,
};
use denon_marantz_proto::telnet::{Event, EventZone, group_queries, state_queries};
use denon_marantz_proto::{
    AudioRestorer, AudysseyDsx, AutoStandby, BluetoothOutputMode, Brand, Channel, ChannelLevel,
    DabDrc, DabStationOrder, DeviceFeature, DeviceFeatures, DigitalCodec, DimmerMode, DiracFilter,
    DynamicCompression, DynamicVolume, EcoMode, FrontSpeakers, HdRadioBand, HdTuningMode,
    HdmiAudioDecode, HdmiOutput, HdmiUpscaler, Illumination, InputMode, MultiEq, MuteLevel,
    NoiseReduction, PanelLock, PictureMode, PliizHeightGain, Power, PowerOnVolume, ProLogicMode,
    ReceiverType, ReferenceLevelOffset, RemoteKey, RoomSize, Setting, Sleep, SpeakerPreset,
    SurroundBackMode, ToneLevel, Trigger, TunerBand, TunerBands, TunerFrequency, TunerInfo,
    TunerMode, ValueRange, VideoProcessingMode, Volume, VolumeDisplay, VolumeMode, Zone,
    ZoneChannelMode, ZoneHdmiAudio,
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
        answer_quiet: Duration::from_millis(200),
        signal_wait_min: Duration::from_millis(300),
        signal_wait_max: Duration::from_millis(1200),
        query_groups: QueryGroup::DEFAULT_ORDER,
    }
}

/// An `Avr` with a telnet connection to `telnet`.
async fn connected(config: TelnetConfig) -> (Avr, common::Fake) {
    connected_with_pause(config, Duration::ZERO).await
}

/// An `Avr` with a telnet connection to `telnet` and the pause after a command.
async fn connected_with_pause(config: TelnetConfig, pause: Duration) -> (Avr, common::Fake) {
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .background_pause(pause)
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
async fn an_audyssey_command_over_http_is_not_read_while_telnet_is_open() {
    // A blank answer would clear what telnet reported, and telnet reports the
    // change itself. Tone control is still read.
    const PAGES: &[(&str, &[u8])] = &[
        ("/goform/formiPhoneAppDirect.xml?PSDYNEQ%20ON", b""),
        ("/goform/formiPhoneAppDirect.xml?PSBAS%20UP", b""),
    ];
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver_with_pages(AVR_X_INFO, PAGES).await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .follow_up_read(Some(quick_follow_up()))
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    status_is(&avr, TelnetStatus::Connected).await;
    let asked = |text: &str| {
        http.requests()
            .iter()
            .filter(|seen| contains(&seen.body, text))
            .count()
    };
    avr.http_send(Zone::Main, Command::DynamicEq(true))
        .await
        .unwrap();
    avr.http_send(Zone::Main, Command::BassUp).await.unwrap();
    wait_until("the tone read", || asked("GetToneControl") > 0).await;
    assert_eq!(asked("GetAudyssey"), 0);
}

#[tokio::test]
async fn settings_commands_over_http_read_no_settings_while_telnet_is_open() {
    const PAGES: &[(&str, &[u8])] = &[
        ("/goform/formiPhoneAppDirect.xml?PSCINEMA%20EQ.OFF", b""),
        ("/goform/formiPhoneAppDirect.xml?SIGAME", b""),
    ];
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver_with_pages(AVR_X_SETTINGS_INFO, PAGES).await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .follow_up_read(Some(quick_follow_up()))
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    status_is(&avr, TelnetStatus::Connected).await;
    let asked = |text: &str| {
        http.requests()
            .iter()
            .filter(|seen| contains(&seen.body, text))
            .count()
    };
    avr.http_send(Zone::Main, Command::Raw("PSCINEMA EQ.OFF"))
        .await
        .unwrap();
    avr.http_send(Zone::Main, Command::SelectSource("GAME"))
        .await
        .unwrap();
    // Longer than the reads after the commands take.
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(asked("GetSurroundParameter") + asked("GetRestorerMode"), 0);
}

#[tokio::test]
async fn send_uses_telnet_while_it_is_open_and_http_otherwise() {
    const PAGES: &[(&str, &[u8])] = &[("/goform/formiPhoneAppDirect.xml?MVUP", b"")];
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver_with_pages(AVR_X_INFO, PAGES).await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .follow_up_read(None)
        .connect()
        .await
        .unwrap();
    let http_commands = || {
        http.requests()
            .iter()
            .filter(|seen| seen.path.starts_with("/goform/formiPhoneAppDirect"))
            .count()
    };
    // Telnet is not open yet.
    avr.send(Zone::Main, Command::VolumeUp).await.unwrap();
    assert_eq!(http_commands(), 1);
    assert!(!telnet.received().contains("MVUP"));

    avr.telnet_connect().await.unwrap();
    status_is(&avr, TelnetStatus::Connected).await;
    avr.send(Zone::Main, Command::VolumeUp).await.unwrap();
    wait_until("the command on telnet", || {
        telnet.received().contains("MVUP")
    })
    .await;
    assert_eq!(http_commands(), 1);

    avr.telnet_disconnect().await;
    avr.zone(Zone::Main).unwrap().volume_up().await.unwrap();
    assert_eq!(http_commands(), 2);
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
    let mut events = avr.telnet_events();
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
    let mut events = avr.telnet_events();
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
async fn blank_audyssey_values_over_http_keep_the_telnet_values() {
    let telnet = FakeTelnet::start().await;
    // A receiver that is calibrated with Dirac answers with blank values.
    let answer = appcommand0300_answer(
        "<param name=\"dynamiceq\" control=\"0\"></param>\
        <param name=\"reflevoffset\" control=\"0\"></param>\
        <param name=\"dynamicvol\" control=\"0\"></param>\
        <param name=\"multeq\" control=\"0\"></param>",
    );
    let answer: &'static str = Box::leak(answer.into_boxed_str());
    let http = receiver_with_audyssey(Some(answer), true).await;
    let avr = builder(&http, ReceiverType::AvrX2016)
        .telnet_config(telnet_config(telnet.port))
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    status_is(&avr, TelnetStatus::Connected).await;
    telnet.send(b"PSDYNEQ OFF\rPSMULTEQ:OFF\rPSREFLEV 0\rPSDYNVOL OFF\r");
    wait_until("the events", || {
        avr.snapshot().device().dynamic_volume.is_some()
    })
    .await;

    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Read
    );
    let state = avr.snapshot();
    let device = state.device();
    assert_eq!(device.dynamic_eq, Some(false));
    assert_eq!(device.multi_eq, Some(MultiEq::Off));
    assert_eq!(
        device.reference_level_offset,
        Some(ReferenceLevelOffset::Db0)
    );
    assert_eq!(device.dynamic_volume, Some(DynamicVolume::Off));
}

#[tokio::test]
async fn a_blank_hdmi_output_over_http_keeps_the_telnet_value() {
    let telnet = FakeTelnet::start().await;
    let answer = appcommand0300_answer("").replace("</rx>", &(output_settings_item("") + "</rx>"));
    let http = receiver_with_info_and_signal_answers(AVR_X_OUTPUT_SETTINGS_INFO, true, move |_| {
        Reply::Body(answer.clone().into_bytes())
    })
    .await;
    let avr = builder(&http, ReceiverType::AvrX2016)
        .telnet_config(telnet_config(telnet.port))
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    status_is(&avr, TelnetStatus::Connected).await;
    telnet.send(b"VSMONI2\r");
    wait_until("the event", || {
        avr.snapshot().device().video_settings.hdmi_output.is_some()
    })
    .await;

    assert_eq!(
        avr.update_signal_and_settings().await.unwrap(),
        PageStatus::Read
    );
    assert_eq!(
        avr.snapshot().device().video_settings.hdmi_output,
        Some(HdmiOutput::Hdmi2)
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
async fn the_connection_fetches_the_chosen_groups_in_the_chosen_order() {
    static CHOSEN: [QueryGroup; 2] = [QueryGroup::Signal, QueryGroup::Core];
    let telnet = FakeTelnet::start().await;
    let config = TelnetConfig {
        query_groups: &CHOSEN,
        ..telnet_config(telnet.port)
    };
    let (_avr, _http) = connected(config).await;
    let expected: Vec<String> = std::iter::once("ZM?")
        .chain(group_queries(Brand::Denon, QueryGroup::Signal))
        .chain(group_queries(Brand::Denon, QueryGroup::Core))
        .map(str::to_owned)
        .collect();
    wait_until("the chosen queries", || {
        sent(&telnet).len() >= expected.len()
    })
    .await;
    // Nothing else follows.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(sent(&telnet), expected);
}

#[tokio::test]
async fn a_group_that_was_left_out_can_be_fetched_later() {
    static CHOSEN: [QueryGroup; 1] = [QueryGroup::Core];
    let telnet = FakeTelnet::start().await;
    let config = TelnetConfig {
        query_groups: &CHOSEN,
        ..telnet_config(telnet.port)
    };
    let (avr, _http) = connected(config).await;
    let core = 1 + group_queries(Brand::Denon, QueryGroup::Core).count();
    wait_until("the core queries", || sent(&telnet).len() >= core).await;
    avr.telnet_refresh_group(QueryGroup::Names).await.unwrap();
    let names: Vec<String> = group_queries(Brand::Denon, QueryGroup::Names)
        .map(str::to_owned)
        .collect();
    wait_until("the names queries", || {
        sent(&telnet).len() >= core + names.len()
    })
    .await;
    assert_eq!(sent(&telnet)[core..], names);
}

/// A connection that fetched nothing at connect, and what it sent so far.
async fn connected_without_queries(telnet: &FakeTelnet) -> Avr {
    static NONE: [QueryGroup; 0] = [];
    let config = TelnetConfig {
        query_groups: &NONE,
        ..telnet_config(telnet.port)
    };
    let (avr, _http) = connected(config).await;
    avr.telnet_connect().await.unwrap();
    wait_until("the first query", || !sent(telnet).is_empty()).await;
    avr
}

#[tokio::test]
async fn ask_sends_exactly_the_values_and_the_raw_queries() {
    let telnet = FakeTelnet::start().await;
    let avr = connected_without_queries(&telnet).await;
    let before = sent(&telnet).len();
    avr.telnet_ask(&[
        Query::DiracFilter.into(),
        Ask::Raw("XYZ ?"),
        Query::DynamicEq.into(),
    ])
    .await
    .unwrap();
    wait_until("the three queries", || sent(&telnet).len() >= before + 3).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        sent(&telnet)[before..],
        ["PSDIRAC ?", "XYZ ?", "PSDYNEQ ?"].map(str::to_owned)
    );
}

#[tokio::test]
async fn ask_sends_nothing_if_one_entry_is_not_valid() {
    let telnet = FakeTelnet::start().await;
    let avr = connected_without_queries(&telnet).await;
    let before = sent(&telnet).len();
    // A Marantz value on a Denon receiver.
    let error = avr
        .telnet_ask(&[Query::DynamicEq.into(), Query::Mdax.into()])
        .await
        .unwrap_err();
    assert!(matches!(error, Error::InvalidRequest(_)), "{error}");
    // A raw text that is not a query.
    let error = avr.telnet_ask(&[Ask::Raw("PWON")]).await.unwrap_err();
    assert!(matches!(error, Error::InvalidRequest(_)), "{error}");
    // A carriage return would start a new command.
    let error = avr
        .telnet_ask(&[Ask::Raw("MV?\rPWSTANDBY?")])
        .await
        .unwrap_err();
    assert!(matches!(error, Error::InvalidCommand(_)), "{error}");
    // Nothing may follow the question mark.
    let error = avr.telnet_ask(&[Ask::Raw("PW? ")]).await.unwrap_err();
    assert!(matches!(error, Error::InvalidRequest(_)), "{error}");
    let error = avr.telnet_ask(&[Ask::Raw("")]).await.unwrap_err();
    assert!(matches!(error, Error::InvalidCommand(_)), "{error}");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(sent(&telnet).len(), before);
}

#[tokio::test]
async fn repeated_asks_do_not_grow_the_queue() {
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver().await;
    let config = TelnetConfig {
        query_groups: &[],
        query_interval: Duration::from_millis(60),
        ..telnet_config(telnet.port)
    };
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(config)
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    // The queue is slower than the requests, so these would pile up. A typed
    // value and the same query as text count as one.
    for _ in 0..3 {
        avr.telnet_ask(&[Query::DynamicEq.into(), Ask::Raw("XYZ ?")])
            .await
            .unwrap();
        avr.telnet_ask(&[Ask::Raw("PSDYNEQ ?")]).await.unwrap();
    }
    wait_until("the raw query", || {
        sent(&telnet).contains(&"XYZ ?".to_owned())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let lines = sent(&telnet);
    let count = |query: &str| lines.iter().filter(|line| line.as_str() == query).count();
    assert_eq!(count("PSDYNEQ ?"), 1, "{lines:?}");
    assert_eq!(count("XYZ ?"), 1, "{lines:?}");
}

#[tokio::test]
async fn ask_needs_an_open_connection() {
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX).connect().await.unwrap();
    let error = avr
        .telnet_ask(&[Query::DiracFilter.into()])
        .await
        .unwrap_err();
    assert!(matches!(error, Error::NotConnected), "{error}");
}

#[tokio::test]
async fn the_tuner_state_is_asked_for_when_the_source_changes_to_the_tuner() {
    let telnet = FakeTelnet::start().await;
    let (_avr, _http) = connected(telnet_config(telnet.port)).await;
    let count = 1 + state_queries(Brand::Denon).count();
    wait_until("all queries", || sent(&telnet).len() >= count).await;
    telnet.send(b"SITUNER\r");
    wait_until("the tuner queries", || {
        sent(&telnet).ends_with(&["TFAN?".to_owned(), "TPAN?".to_owned(), "TMAN?".to_owned()])
    })
    .await;
}

#[tokio::test]
async fn the_hd_radio_state_is_asked_for_when_the_source_changes_to_hd_radio() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let count = 1 + state_queries(Brand::Denon).count();
    wait_until("all queries", || sent(&telnet).len() >= count).await;
    telnet.send(b"SIHDRADIO\r");
    let hd = ["HD?", "TFHD?", "TPHD?", "TMHD?"].map(str::to_owned);
    wait_until("the HD Radio queries", || sent(&telnet).ends_with(&hd)).await;
    avr.tuner().request_hd().await.unwrap();
    avr.tuner().request_presets().await.unwrap();
    let mut wanted = hd.to_vec();
    wanted.push("OPTPN ?".to_owned());
    wait_until("the requests", || sent(&telnet).ends_with(&wanted)).await;
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
    let mut events = avr.telnet_events();
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
    avr.telnet_send(Zone::Main, Command::VolumeUp)
        .await
        .unwrap();
    avr.telnet_send(Zone::Zone2, Command::Mute(true))
        .await
        .unwrap();
    avr.telnet_send(Zone::Main, Command::SelectSource("GAME"))
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
    avr.telnet_send(Zone::Main, Command::VolumeDown)
        .await
        .unwrap();
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
        ..telnet_config(telnet.port)
    };
    let (avr, _http) = connected_with_pause(config, Duration::from_millis(300)).await;
    wait_until("a few queries", || sent(&telnet).len() >= 3).await;
    avr.telnet_send(Zone::Main, Command::VolumeUp)
        .await
        .unwrap();
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
        avr.telnet_send(Zone::Main, Command::VolumeUp)
            .await
            .unwrap();
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
    let error = avr
        .telnet_send(Zone::Main, Command::VolumeUp)
        .await
        .unwrap_err();
    assert!(error.is_not_connected(), "{error:?}");
    assert!(avr.telnet_refresh().await.unwrap_err().is_not_connected());

    avr.telnet_connect().await.unwrap();
    avr.telnet_send(Zone::Main, Command::VolumeUp)
        .await
        .unwrap();

    // While the task connects again.
    telnet.close();
    status_is(&avr, TelnetStatus::Disconnected).await;
    let error = avr
        .telnet_send(Zone::Main, Command::VolumeUp)
        .await
        .unwrap_err();
    assert!(error.is_not_connected(), "{error:?}");

    // After a disconnect.
    status_is(&avr, TelnetStatus::Connected).await;
    avr.telnet_disconnect().await;
    let error = avr
        .telnet_send(Zone::Main, Command::VolumeUp)
        .await
        .unwrap_err();
    assert!(error.is_not_connected(), "{error:?}");
}

#[tokio::test]
async fn invalid_commands_are_refused_before_anything_is_sent() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    wait_until("the queries", || sent(&telnet).len() >= 79).await;
    for text in ["", "A\rMVUP", "A\nB"] {
        let error = avr
            .telnet_send(Zone::Main, Command::Raw(text))
            .await
            .unwrap_err();
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
    let waiting = avr.telnet_send(Zone::Main, Command::VolumeUp);
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
    assert!(error.is_unsupported());
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

#[tokio::test]
async fn remote_keys_use_the_category_of_the_zone() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let main = avr.zone(Zone::Main).unwrap();
    let zone2 = avr.zone(Zone::Zone2).unwrap();
    main.press(RemoteKey::NextTrack).await.unwrap();
    zone2.press(RemoteKey::PageUp).await.unwrap();
    zone2.press_denon_code(5).await.unwrap();
    let error = zone2.press(RemoteKey::HdmiCecOn).await.unwrap_err();
    assert!(error.is_unsupported(), "{error:?}");
    let error = main.press_marantz_code("6012").await.unwrap_err();
    assert!(error.is_unsupported(), "{error:?}");
    wait_until("the keys", || commands_sent(&telnet).len() >= 3).await;
    assert_eq!(
        commands_sent(&telnet),
        ["RCKSK0470776", "RCKSK0430945", "RCKSK0430005"]
    );
}

#[tokio::test]
async fn video_methods_send_their_commands_and_events_fill_the_state() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let video = avr.video();
    video
        .set_processing_mode(VideoProcessingMode::Movie)
        .await
        .unwrap();
    video.set_hdmi_output(HdmiOutput::Hdmi2).await.unwrap();
    video.set_hdmi_audio(HdmiAudioDecode::Tv).await.unwrap();
    video.set_vertical_stretch(true).await.unwrap();
    video.set_hdmi_upscaler(HdmiUpscaler::Off).await.unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 5).await;
    assert_eq!(
        commands_sent(&telnet),
        ["VSVPMMOVI", "VSMONI2", "VSAUDIO TV", "VSVST ON", "VSSCHOFF"]
    );
    telnet.send(b"VSVPMBYP\rVSAUDIO AMP\r");
    wait_until("the video values", || {
        avr.snapshot().device().video_settings.hdmi_audio.is_some()
    })
    .await;
    let video = avr.snapshot().device().video_settings;
    assert_eq!(video.processing_mode, Some(VideoProcessingMode::Bypass));
    assert_eq!(video.hdmi_audio, Some(HdmiAudioDecode::Amp));
}

#[tokio::test]
async fn triggers_are_sent_and_read() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    avr.device().set_trigger(Trigger::Two, true).await.unwrap();
    avr.device().set_trigger(Trigger::One, false).await.unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 2).await;
    assert_eq!(commands_sent(&telnet), ["TR2 ON", "TR1 OFF"]);
    telnet.send(b"TR2 ON\rTR3 OFF\r");
    wait_until("the trigger values", || {
        avr.snapshot().device().trigger(Trigger::Three).is_some()
    })
    .await;
    let state = avr.snapshot();
    assert_eq!(state.device().trigger(Trigger::Two), Some(true));
    assert_eq!(state.device().trigger(Trigger::Three), Some(false));
}

#[tokio::test]
async fn presets_use_the_form_that_the_receiver_reports() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let tuner = avr.tuner();
    telnet.send(b"TPANA2\rTPHDB1\r");
    wait_until("the lettered forms", || {
        let state = avr.snapshot();
        let tuner = &state.device().tuner;
        tuner.presets_lettered == Some(true) && tuner.hd.presets_lettered == Some(true)
    })
    .await;
    tuner.select_preset(3).await.unwrap();
    tuner.store_preset(56).await.unwrap();
    tuner.select_hd_preset(9).await.unwrap();
    tuner.store_hd_preset(1).await.unwrap();
    telnet.send(b"TPAN02\r");
    wait_until("the numbered form", || {
        avr.snapshot().device().tuner.presets_lettered == Some(false)
    })
    .await;
    tuner.select_preset(3).await.unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 5).await;
    assert_eq!(
        commands_sent(&telnet),
        ["TPANA3", "TPANMEMG8", "TPHDB1", "TPHDMEMA1", "TPAN03"]
    );
}

#[tokio::test]
async fn tuner_methods_send_their_commands_and_events_fill_the_state() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let tuner = avr.tuner();
    tuner.set_band(TunerBand::Fm).await.unwrap();
    tuner.frequency_up().await.unwrap();
    tuner.select_preset(3).await.unwrap();
    tuner.store_preset(12).await.unwrap();
    tuner.select_lettered_preset(9).await.unwrap();
    tuner.store_lettered_preset(56).await.unwrap();
    let error = tuner.select_preset(57).await.unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    let error = tuner.select_lettered_preset(0).await.unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    wait_until("the commands", || commands_sent(&telnet).len() >= 6).await;
    assert_eq!(
        commands_sent(&telnet),
        [
            "TMANFM",
            "TFANUP",
            "TPAN03",
            "TPANMEM12",
            "TPANB1",
            "TPANMEMG8"
        ]
    );
    tuner.request().await.unwrap();
    wait_until("the queries", || {
        sent(&telnet).contains(&"TMAN?".to_owned())
    })
    .await;
    let lines = sent(&telnet);
    let first = lines.iter().position(|line| line == "TFAN?").unwrap();
    assert_eq!(lines[first..first + 3], ["TFAN?", "TPAN?", "TMAN?"]);
    telnet.send(b"TFAN008750\rTPAN05\r");
    wait_until("the tuner values", || {
        avr.snapshot().device().tuner.preset.is_some()
    })
    .await;
    let state = avr.snapshot();
    assert_eq!(state.device().tuner.preset, Some(5));
    assert_eq!(state.device().tuner.band, Some(TunerBand::Fm));
}

#[tokio::test]
async fn zone_setting_methods_send_their_commands() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let zone2 = avr.zone(Zone::Zone2).unwrap();
    zone2
        .set_bass(ToneLevel::from_db(8).unwrap())
        .await
        .unwrap();
    zone2.treble_down().await.unwrap();
    zone2.set_left_level(2).await.unwrap();
    zone2.set_high_pass_filter(true).await.unwrap();
    zone2.set_channel_mode(ZoneChannelMode::Mono).await.unwrap();
    zone2.set_hdmi_audio(ZoneHdmiAudio::Pcm).await.unwrap();
    zone2
        .set_volume_limit(Volume::from_db_rounded(-10.0))
        .await
        .unwrap();
    zone2.set_volume_limit(None).await.unwrap();
    zone2
        .set_power_on_volume(PowerOnVolume::Last)
        .await
        .unwrap();
    zone2.set_mute_level(MuteLevel::Minus20Db).await.unwrap();
    zone2
        .set_volume_mode(VolumeMode::Fixed(Volume::from_db_rounded(-70.0).unwrap()))
        .await
        .unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 11).await;
    assert_eq!(
        commands_sent(&telnet),
        [
            "Z2PSBAS 58",
            "Z2PSTRE DOWN",
            "Z2CVFL 52",
            "Z2HPFON",
            "Z2CSMONO",
            "Z2HDA PCM",
            "SSVCTZ2SLIM 070",
            "SSVCTZ2SLIM OFF",
            "SSVCTZ2SPON LAS",
            "SSVCTZ2SMLV 060",
            "SSVCTZ2SLEV 10",
        ]
    );
}

#[tokio::test]
async fn main_zone_volume_settings_send_their_commands() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let main = avr.zone(Zone::Main).unwrap();
    main.set_volume_limit(Volume::from_db_rounded(-20.0))
        .await
        .unwrap();
    main.set_volume_limit(None).await.unwrap();
    main.set_power_on_volume(PowerOnVolume::Level(
        Volume::from_db_rounded(-45.0).unwrap(),
    ))
    .await
    .unwrap();
    main.set_mute_level(MuteLevel::Minus40Db).await.unwrap();
    main.set_volume_display(VolumeDisplay::Absolute)
        .await
        .unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 5).await;
    assert_eq!(
        commands_sent(&telnet),
        [
            "SSVCTZMALIM 60",
            "SSVCTZMALIM OFF",
            "SSVCTZMAPON 35",
            "SSVCTZMAMLV 040",
            "SSVCTZMADIS ABS",
        ]
    );
}

#[tokio::test]
async fn zone_setting_methods_refuse_what_the_zone_does_not_have() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let main = avr.zone(Zone::Main).unwrap();
    let error = main.bass_up().await.unwrap_err();
    assert!(error.is_unsupported(), "{error:?}");
    let error = main
        .set_volume_mode(VolumeMode::Variable)
        .await
        .unwrap_err();
    assert!(error.is_unsupported(), "{error:?}");
    let zone2 = avr.zone(Zone::Zone2).unwrap();
    let error = zone2
        .set_volume_display(VolumeDisplay::Absolute)
        .await
        .unwrap_err();
    assert!(error.is_unsupported(), "{error:?}");
    let error = zone2.set_left_level(13).await.unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    let half = Volume::from_db_rounded(-9.5);
    let error = zone2.set_volume_limit(half).await.unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    // The main zone has its own range for the tone control.
    let error = avr
        .tone()
        .set_bass(ToneLevel::from_db(8).unwrap())
        .await
        .unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        commands_sent(&telnet).is_empty(),
        "{:?}",
        commands_sent(&telnet)
    );
}

#[tokio::test]
async fn zone_bass_up_to_10_db_fills_the_state() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    telnet.send(b"Z2PSBAS 58\rZ2PSTRE 42\r");
    wait_until("the zone tone", || {
        avr.snapshot().zone(Zone::Zone2).treble.is_some()
    })
    .await;
    let state = avr.snapshot();
    assert_eq!(state.zone(Zone::Zone2).bass, ToneLevel::from_db(8));
    assert_eq!(state.zone(Zone::Zone2).treble, ToneLevel::from_db(-8));
}

#[tokio::test]
async fn device_methods_send_their_commands() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let device = avr.device();
    device.set_dimmer(DimmerMode::Dim).await.unwrap();
    device.dimmer_next().await.unwrap();
    device.set_eco_mode(EcoMode::Auto).await.unwrap();
    device.set_remote_lock(true).await.unwrap();
    device.set_video_select(false).await.unwrap();
    device.select_video_source("GAME").await.unwrap();
    device.select_record_source("CD").await.unwrap();
    device
        .set_panel_lock(PanelLock::PanelAndVolume)
        .await
        .unwrap();
    device.set_panel_lock(PanelLock::Off).await.unwrap();
    device.channel_level_menu().await.unwrap();
    device.show_status().await.unwrap();
    device.restart_network().await.unwrap();
    avr.zone(Zone::Main)
        .unwrap()
        .sound_mode_next()
        .await
        .unwrap();
    avr.zone(Zone::Main)
        .unwrap()
        .sound_mode_previous()
        .await
        .unwrap();
    let error = avr
        .zone(Zone::Zone2)
        .unwrap()
        .sound_mode_next()
        .await
        .unwrap_err();
    assert!(error.is_unsupported(), "{error:?}");
    wait_until("the commands", || commands_sent(&telnet).len() >= 14).await;
    assert_eq!(
        commands_sent(&telnet),
        [
            "DIM DIM",
            "DIM SEL",
            "ECOAUTO",
            "SYREMOTE LOCK ON",
            "SVOFF",
            "SVGAME",
            "SRCD",
            "SYPANEL+V LOCK ON",
            "SYPANEL LOCK OFF",
            "MNCHL",
            "RCSHP0230030",
            "NSRBT",
            "MSRIGHT",
            "MSLEFT",
        ]
    );
}

#[tokio::test]
async fn bluetooth_and_quick_select_are_sent_and_read() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let device = avr.device();
    device.set_bluetooth_transmitter(true).await.unwrap();
    device
        .set_bluetooth_output(BluetoothOutputMode::BluetoothOnly)
        .await
        .unwrap();
    // The illumination is for Marantz receivers.
    let error = device
        .set_illumination(Illumination::Dim)
        .await
        .unwrap_err();
    assert!(error.is_unsupported(), "{error:?}");
    let main = avr.zone(Zone::Main).unwrap();
    let zone2 = avr.zone(Zone::Zone2).unwrap();
    main.select_quick_select(3).await.unwrap();
    zone2.store_quick_select(2).await.unwrap();
    let error = main.select_quick_select(7).await.unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    wait_until("the commands", || commands_sent(&telnet).len() >= 4).await;
    assert_eq!(
        commands_sent(&telnet),
        ["BTTX ON", "BTTX BT", "MSQUICK3", "Z2QUICK2 MEMORY"]
    );
    telnet.send(b"BTTX OFF\rBTTX SP\rMSQUICK1\rZ2QUICK4\r");
    wait_until("the values", || {
        avr.snapshot().zone(Zone::Zone2).quick_select.is_some()
    })
    .await;
    let state = avr.snapshot();
    assert_eq!(state.device().bluetooth_transmitter, Some(false));
    assert_eq!(
        state.device().bluetooth_output,
        Some(BluetoothOutputMode::BluetoothAndSpeakers)
    );
    assert_eq!(state.zone(Zone::Main).quick_select, Some(1));
    assert_eq!(state.zone(Zone::Zone2).quick_select, Some(4));
}

#[tokio::test]
async fn picture_settings_are_sent_and_read() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let video = avr.video();
    video.set_picture_mode(PictureMode::Custom).await.unwrap();
    video.set_contrast(-5).await.unwrap();
    video.brightness_up().await.unwrap();
    video
        .set_noise_reduction(NoiseReduction::Mid)
        .await
        .unwrap();
    video.set_enhancer(12).await.unwrap();
    let error = video.set_enhancer(13).await.unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    let error = video.set_contrast(51).await.unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    wait_until("the commands", || commands_sent(&telnet).len() >= 5).await;
    assert_eq!(
        commands_sent(&telnet),
        ["PVCTM", "PVCN 045", "PVBR UP", "PVDNR MID", "PVENH 12"]
    );
    telnet.send(b"PVMOV\rPVCN 060\rPVDNR HI\r");
    wait_until("the picture values", || {
        avr.snapshot()
            .device()
            .picture_settings
            .noise_reduction
            .is_some()
    })
    .await;
    let picture = avr.snapshot().device().picture_settings;
    assert_eq!(picture.mode, Some(PictureMode::Movie));
    assert_eq!(picture.contrast, Some(10));
    assert_eq!(picture.noise_reduction, Some(NoiseReduction::High));
}

#[tokio::test]
async fn digital_codec_and_favorites_are_sent_and_read() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let device = avr.device();
    device.set_digital_codec(DigitalCodec::Dts).await.unwrap();
    device.select_favorite(2).await.unwrap();
    device.store_favorite(3).await.unwrap();
    let error = device.select_favorite(5).await.unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    wait_until("the commands", || commands_sent(&telnet).len() >= 3).await;
    assert_eq!(
        commands_sent(&telnet),
        ["DCDTS", "ZMFAVORITE2", "ZMFAVORITE3 MEMORY"]
    );
    telnet.send(b"DCPCM\r");
    wait_until("the digital codec", || {
        avr.snapshot().device().digital_codec.is_some()
    })
    .await;
    assert_eq!(
        avr.snapshot().device().digital_codec,
        Some(DigitalCodec::Pcm)
    );
}

#[tokio::test]
async fn sound_steps_and_the_effect_level_are_sent_and_read() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let sound = avr.sound();
    sound.lfe_level_up().await.unwrap();
    sound.center_level_down().await.unwrap();
    sound.subwoofer_level_up(2).await.unwrap();
    sound.audio_delay_down().await.unwrap();
    sound.dialog_control_up().await.unwrap();
    sound.set_effect_level(10).await.unwrap();
    let error = sound.subwoofer_level_down(5).await.unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    let error = sound.set_effect_level(0).await.unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    wait_until("the commands", || commands_sent(&telnet).len() >= 6).await;
    assert_eq!(
        commands_sent(&telnet),
        [
            "PSLFE UP",
            "PSCLV DOWN",
            "PSSWL2 UP",
            "PSDELAY DOWN",
            "PSDIC UP",
            "PSEFF 10",
        ]
    );
    telnet.send(b"PSEFF 12\rPSEFF ON\r");
    wait_until("the effect", || {
        avr.snapshot().device().sound_settings.effect.is_some()
    })
    .await;
    let settings = avr.snapshot().device().sound_settings.clone();
    assert_eq!(settings.effect_level, Some(12));
    assert_eq!(settings.effect, Some(true));
}

#[tokio::test]
async fn dialog_level_dsx_and_height_gain_are_sent() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let sound = avr.sound();
    sound.set_dialog_level_adjust(true).await.unwrap();
    sound.set_dialog_level(-3).await.unwrap();
    sound.dialog_level_up().await.unwrap();
    sound.set_audyssey_dsx(AudysseyDsx::Wide).await.unwrap();
    sound.set_stage_width(2).await.unwrap();
    sound.stage_width_down().await.unwrap();
    sound.set_stage_height(-1).await.unwrap();
    sound.stage_height_up().await.unwrap();
    sound
        .set_height_gain(PliizHeightGain::Medium)
        .await
        .unwrap();
    sound
        .set_pro_logic_mode(ProLogicMode::ProLogic)
        .await
        .unwrap();
    sound
        .set_surround_back_mode(SurroundBackMode::MatrixOn)
        .await
        .unwrap();
    sound.set_front_height(true).await.unwrap();
    sound
        .set_dynamic_compression(DynamicCompression::Low)
        .await
        .unwrap();
    sound.set_auto_flag_detect(false).await.unwrap();
    let error = sound.set_dialog_level(13).await.unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    let error = sound.set_stage_width(-11).await.unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    wait_until("the commands", || commands_sent(&telnet).len() >= 14).await;
    assert_eq!(
        commands_sent(&telnet),
        [
            "PSDIL ON",
            "PSDIL 47",
            "PSDIL UP",
            "PSDSX ONW",
            "PSSTW 52",
            "PSSTW DOWN",
            "PSSTH 49",
            "PSSTH UP",
            "PSPHG MID",
            "PSMODE:PRO LOGIC",
            "PSSB:MTRX ON",
            "PSFH:ON",
            "PSDCO LOW",
            "PSAFD OFF",
        ]
    );
}

#[tokio::test]
async fn the_factory_reset_needs_the_last_key_once() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let device = avr.device();
    let old = device.reset_key();
    let key = device.reset_key();
    // The old key stopped being valid when the new one came.
    let error = device.factory_reset(old).await.unwrap_err();
    assert!(matches!(error, Error::ResetKeyInvalid), "{error:?}");
    assert!(error.is_invalid_input());
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        commands_sent(&telnet).is_empty(),
        "{:?}",
        commands_sent(&telnet)
    );
    device.factory_reset(key).await.unwrap();
    wait_until("the reset", || !commands_sent(&telnet).is_empty()).await;
    assert_eq!(commands_sent(&telnet), ["SYRST"]);
    // A key works once.
    let used = device.reset_key();
    device.factory_reset(used).await.unwrap();
    wait_until("the second reset", || commands_sent(&telnet).len() >= 2).await;
}

#[tokio::test]
async fn auto_standby_and_sleep_work_in_every_zone() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let main = avr.zone(Zone::Main).unwrap();
    let zone2 = avr.zone(Zone::Zone2).unwrap();
    main.set_auto_standby(AutoStandby::Minutes30).await.unwrap();
    main.set_sleep(Sleep::minutes(45).unwrap()).await.unwrap();
    zone2.set_auto_standby(AutoStandby::Hours4).await.unwrap();
    zone2.set_sleep(Sleep::Off).await.unwrap();
    let error = zone2
        .set_auto_standby(AutoStandby::Minutes15)
        .await
        .unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    wait_until("the commands", || commands_sent(&telnet).len() >= 4).await;
    assert_eq!(
        commands_sent(&telnet),
        ["STBY30M", "SLP045", "Z2STBY4H", "Z2SLPOFF"]
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
    assert_eq!(state.zone(Zone::Main).treble, Some(ToneLevel::MAIN_MIN));
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
    tone.set_treble(ToneLevel::MAIN_MAX).await.unwrap();
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
    sound.set_input_mode(InputMode::Hdmi).await.unwrap();
    sound.set_tactile_transducer(true).await.unwrap();
    sound.set_front_speakers(FrontSpeakers::B).await.unwrap();
    sound.set_speaker_preset(SpeakerPreset::Two).await.unwrap();
    sound.tactile_level_up().await.unwrap();
    sound.set_tactile_low_pass(60).await.unwrap();
    let level = ChannelLevel::from_half_db(-1).unwrap();
    sound.set_subwoofer_level(2, level).await.unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 13).await;
    assert_eq!(
        commands_sent(&telnet),
        [
            "PSRSZ L",
            "PSRSTR OFF",
            "PSGEQ ON",
            "PSCINEMA EQ.OFF",
            "PSDELAY 005",
            "PSIMAXHPF 080",
            "SDHDMI",
            "SSTTR ON",
            "PSFRONT SPB",
            "SPPR 2",
            "SSTTRLEV UP",
            "SSTTRLPF 060",
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
        sound.set_imax_high_pass(65).await.unwrap_err(),
        sound.set_subwoofer_level(5, level).await.unwrap_err(),
    ];
    for error in errors {
        assert!(matches!(error, Error::NumberOutOfRange { .. }), "{error:?}");
        assert!(error.is_invalid_input());
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(commands_sent(&telnet).is_empty(), "{:?}", sent(&telnet));
}

#[tokio::test]
async fn center_width_uses_the_reported_name() {
    // Before the receiver reports a name, both names are sent.
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    avr.sound().set_center_width(3).await.unwrap();
    avr.sound().center_width_up().await.unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 4).await;
    assert_eq!(
        commands_sent(&telnet),
        ["PSCEN 03", "PSCEW 03", "PSCEN UP", "PSCEW UP"]
    );
    // After a report, only that name is sent.
    for (report, expected) in [
        (&b"PSCEW 02\r"[..], ["PSCEW 04", "PSCEW DOWN"]),
        (&b"PSCEN 02\r"[..], ["PSCEN 04", "PSCEN DOWN"]),
    ] {
        let telnet = FakeTelnet::start().await;
        let (avr, _http) = connected(telnet_config(telnet.port)).await;
        telnet.send(report);
        wait_until("the state", || {
            avr.snapshot()
                .device()
                .sound_settings
                .center_width_name
                .is_some()
        })
        .await;
        avr.sound().set_center_width(4).await.unwrap();
        avr.sound().center_width_down().await.unwrap();
        wait_until("the commands", || commands_sent(&telnet).len() >= 2).await;
        assert_eq!(commands_sent(&telnet), expected);
    }
}

#[tokio::test]
async fn channel_volumes_are_sent_and_read() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let sound = avr.sound();
    let level = ChannelLevel::from_half_db(-3).unwrap();
    sound
        .set_channel_volume(Channel::FrontLeft, level)
        .await
        .unwrap();
    sound.channel_volume_up(Channel::Center).await.unwrap();
    sound
        .channel_volume_down(Channel::TopFrontLeft)
        .await
        .unwrap();
    sound.reset_channel_volumes().await.unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 4).await;
    assert_eq!(
        commands_sent(&telnet),
        ["CVFL 485", "CVC UP", "CVTFL DOWN", "CVZRL"]
    );
    telnet.send(b"CVFL 485\rCVSW2 50\rCV END\r");
    wait_until("the state", || {
        avr.snapshot().device().channel_volumes.len() == 2
    })
    .await;
    let state = avr.snapshot();
    let levels = &state.device().channel_volumes;
    assert_eq!(levels[0], (Channel::FrontLeft, level));
    assert_eq!(levels[1].0, Channel::Subwoofer2);
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
    assert!(error.is_blocked());
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
        assert!(error.is_blocked());
    }
    // Any Dirac filter blocks it, also one that is not Dirac ART.
    telnet.send(b"OPDLSSL1TYP BC\rPSDIRAC 1\r");
    wait_until("the other filter", || {
        avr.snapshot().device().dirac_art_active() == Some(false)
    })
    .await;
    let error = tone.set_enabled(true).await.unwrap_err();
    assert!(
        matches!(error, Error::ToneControlBlocked(ToneBlocker::DiracFilter)),
        "{error:?}"
    );
    // Dynamic EQ does not block it: that is not confirmed for a receiver.
    telnet.send(b"PSDIRAC OFF\rPSDYNEQ ON\r");
    wait_until("dynamic eq", || {
        avr.snapshot().device().dynamic_eq == Some(true)
    })
    .await;
    tone.set_enabled(true).await.unwrap();
    wait_until("the command", || !commands_sent(&telnet).is_empty()).await;
    assert_eq!(commands_sent(&telnet), ["PSTONE CTRL ON"]);
    // Turning it off is never blocked.
    tone.set_enabled(false).await.unwrap();
    wait_until("the command", || commands_sent(&telnet).len() >= 2).await;
    assert_eq!(commands_sent(&telnet)[1], "PSTONE CTRL OFF");
    // If the state says that tone control is on, the levels work.
    telnet.send(b"PSTONE CTRL ON\r");
    wait_until("tone control on", || {
        avr.snapshot().device().tone_control == Some(true)
    })
    .await;
    tone.set_bass_db(3).await.unwrap();
    wait_until("the level", || commands_sent(&telnet).len() >= 3).await;
    assert_eq!(commands_sent(&telnet)[2], "PSBAS 53");
}

#[tokio::test]
async fn audyssey_settings_are_not_sent_while_a_dirac_filter_is_selected() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let audyssey = avr.audyssey();
    // Without a Dirac filter the commands go out.
    audyssey.set_dynamic_eq(true).await.unwrap();
    wait_until("the command", || !commands_sent(&telnet).is_empty()).await;
    assert_eq!(commands_sent(&telnet), ["PSDYNEQ ON"]);

    telnet.send(b"PSDIRAC 1\r");
    wait_until("the filter", || {
        avr.snapshot().device().dirac_filter == Some(DiracFilter::Slot1)
    })
    .await;
    let blocked = [
        audyssey.set_dynamic_eq(false).await,
        audyssey
            .set_reference_level_offset(ReferenceLevelOffset::Db0)
            .await,
        audyssey.set_dynamic_volume(DynamicVolume::Light).await,
        audyssey.set_lfc(true).await,
        audyssey.set_containment_level(3).await,
        audyssey.containment_amount_up().await,
        audyssey.containment_amount_down().await,
    ];
    for result in blocked {
        let error = result.unwrap_err();
        assert!(matches!(error, Error::AudysseyBlockedByDirac), "{error:?}");
        assert!(error.is_blocked());
    }
    assert_eq!(commands_sent(&telnet), ["PSDYNEQ ON"]);
    // Selecting a MultEQ mode is the way back to Audyssey: it is sent.
    audyssey.set_multi_eq(MultiEq::Reference).await.unwrap();
    wait_until("the mode", || commands_sent(&telnet).len() >= 2).await;
    assert_eq!(commands_sent(&telnet)[1], "PSMULTEQ:AUDYSSEY");
    // With Dirac off again, the others go out.
    telnet.send(b"PSDIRAC OFF\r");
    wait_until("the filter is off", || {
        avr.snapshot().device().dirac_filter == Some(DiracFilter::Off)
    })
    .await;
    audyssey.set_lfc(true).await.unwrap();
    wait_until("the command", || commands_sent(&telnet).len() >= 3).await;
    assert_eq!(commands_sent(&telnet)[2], "PSLFC ON");
}

#[tokio::test]
async fn setting_status_agrees_with_the_setters() {
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver_with_info(AVR_X_DIRAC_INFO).await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    let features = [
        Feature::ToneControl,
        Feature::Audyssey,
        Feature::ReferenceLevelOffset,
    ];
    // Nothing is known yet.
    for feature in features {
        assert_eq!(avr.setting_status(feature), SettingStatus::Unknown);
    }
    assert_eq!(avr.setting_status(Feature::Dirac), SettingStatus::Available);

    // Dirac off, Dynamic EQ off: only the reference level offset is blocked.
    telnet.send(b"PSDIRAC OFF\rPSDYNEQ OFF\r");
    wait_until("the state", || {
        avr.snapshot().device().dynamic_eq == Some(false)
            && avr.snapshot().device().dirac_filter == Some(DiracFilter::Off)
    })
    .await;
    assert_eq!(
        avr.setting_status(Feature::ToneControl),
        SettingStatus::Available
    );
    assert_eq!(
        avr.setting_status(Feature::Audyssey),
        SettingStatus::Available
    );
    assert_eq!(
        avr.setting_status(Feature::ReferenceLevelOffset),
        SettingStatus::Blocked(Blocker::DynamicEqOff)
    );
    avr.tone().set_enabled(true).await.unwrap();
    avr.audyssey().set_dynamic_eq(true).await.unwrap();
    let error = avr
        .audyssey()
        .set_reference_level_offset(ReferenceLevelOffset::Db0)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::DynamicEqOff), "{error:?}");
    wait_until("the commands", || commands_sent(&telnet).len() >= 2).await;

    // Dynamic EQ on: the reference level offset is not blocked.
    telnet.send(b"PSDYNEQ ON\r");
    wait_until("dynamic eq", || {
        avr.snapshot().device().dynamic_eq == Some(true)
    })
    .await;
    assert_eq!(
        avr.setting_status(Feature::ReferenceLevelOffset),
        SettingStatus::Available
    );
    avr.audyssey()
        .set_reference_level_offset(ReferenceLevelOffset::Db0)
        .await
        .unwrap();
    wait_until("the command", || commands_sent(&telnet).len() >= 3).await;

    // A Dirac filter blocks all three, and the setters refuse.
    telnet.send(b"PSDIRAC 1\r");
    wait_until("the filter", || {
        avr.snapshot().device().dirac_filter == Some(DiracFilter::Slot1)
    })
    .await;
    for feature in features {
        let status = avr.setting_status(feature);
        assert_eq!(status, SettingStatus::Blocked(Blocker::DiracFilter));
        assert!(!status.can_send());
    }
    let error = avr.tone().set_enabled(true).await.unwrap_err();
    assert!(
        matches!(error, Error::ToneControlBlocked(ToneBlocker::DiracFilter)),
        "{error:?}"
    );
    let error = avr.audyssey().set_dynamic_eq(true).await.unwrap_err();
    assert!(matches!(error, Error::AudysseyBlockedByDirac), "{error:?}");
    let error = avr
        .audyssey()
        .set_reference_level_offset(ReferenceLevelOffset::Db0)
        .await
        .unwrap_err();
    assert!(matches!(error, Error::AudysseyBlockedByDirac), "{error:?}");
    assert_eq!(commands_sent(&telnet).len(), 3);
}

#[tokio::test]
async fn setting_status_without_dirac_needs_no_filter_value() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    assert_eq!(
        avr.setting_status(Feature::Dirac),
        SettingStatus::Unsupported
    );
    // The receiver never reports a filter, so these are not left unknown.
    assert_eq!(
        avr.setting_status(Feature::ToneControl),
        SettingStatus::Available
    );
    assert_eq!(
        avr.setting_status(Feature::Audyssey),
        SettingStatus::Available
    );
    // The offset still depends on Dynamic EQ.
    assert_eq!(
        avr.setting_status(Feature::ReferenceLevelOffset),
        SettingStatus::Unknown
    );
}

const DISTANCES_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-speakers-distances.xml");
const CHANNEL_LEVELS_PAGE: &[u8] = include_bytes!(
    "../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-option-channel-levels.xml"
);

#[tokio::test]
async fn the_channel_level_page_is_not_asked_for_while_dirac_art_is_active() {
    let receiver = avr_x_receiver().await;
    let web_interface = common::Fake::start(|seen| match seen.path.as_str() {
        "/ajax/speakers/get_config?type=4" => Reply::Body(DISTANCES_PAGE.to_vec()),
        "/ajax/option/get_config?type=3" => Reply::Body(CHANNEL_LEVELS_PAGE.to_vec()),
        _ => Reply::Status(404),
    })
    .await;
    let telnet = FakeTelnet::start().await;
    let avr = builder(&receiver, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .web_interface_port(web_interface.port)
        .web_interface_config(common::config())
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    let asked = |web_interface: &common::Fake| {
        web_interface
            .requests()
            .iter()
            .filter(|seen| seen.path.ends_with("type=3"))
            .count()
    };
    // The kind of the filter is not known: the page is asked for.
    let result = avr.update_speaker_details().await.unwrap();
    assert_eq!(result.channel_levels, PageStatus::Read);
    assert_eq!(asked(&web_interface), 1);
    // Dirac ART is active: it is not asked for, and the levels stay.
    telnet.send(b"OPDLSSL2TYP ART\rPSDIRAC 2\r");
    wait_until("the filter", || {
        avr.snapshot().device().dirac_art_active() == Some(true)
    })
    .await;
    let result = avr.update_speaker_details().await.unwrap();
    assert_eq!(
        result.channel_levels,
        PageStatus::Skipped(SkipReason::DiracArt)
    );
    assert_eq!(result.distances, PageStatus::Read);
    assert_eq!(asked(&web_interface), 1);
    assert_eq!(avr.snapshot().device().active_channels.len(), 13);
    // A filter of another kind does not stop it.
    telnet.send(b"OPDLSSL1TYP BC\rPSDIRAC 1\r");
    wait_until("the other filter", || {
        avr.snapshot().device().dirac_art_active() == Some(false)
    })
    .await;
    let result = avr.update_speaker_details().await.unwrap();
    assert_eq!(result.channel_levels, PageStatus::Read);
    assert_eq!(asked(&web_interface), 2);
}

#[tokio::test]
async fn dirac_needs_a_receiver_that_supports_it() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let error = avr.dirac().unwrap_err();
    assert!(matches!(error, Error::DiracNotSupported), "{error:?}");
    assert!(error.is_unsupported());

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
async fn the_other_tuner_methods_send_their_commands() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let tuner = avr.tuner();
    tuner.frequency_down().await.unwrap();
    tuner
        .set_frequency(TunerFrequency::from_hundredths(8750).unwrap())
        .await
        .unwrap();
    tuner.set_mode(TunerMode::Manual).await.unwrap();
    tuner.preset_up().await.unwrap();
    tuner.preset_down().await.unwrap();
    let error = tuner.store_preset(0).await.unwrap_err();
    assert!(error.is_invalid_input(), "{error:?}");
    wait_until("the commands", || commands_sent(&telnet).len() >= 5).await;
    assert_eq!(
        commands_sent(&telnet),
        ["TFANDOWN", "TFAN008750", "TMANMANUAL", "TPANUP", "TPANDOWN"]
    );
}

#[tokio::test]
async fn hd_tuner_methods_send_their_commands() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let tuner = avr.tuner();
    tuner.hd_frequency_up().await.unwrap();
    tuner.hd_frequency_down().await.unwrap();
    tuner.select_hd_multicast(2).await.unwrap();
    tuner.set_hd_band(HdRadioBand::Fm).await.unwrap();
    tuner.set_hd_mode(HdTuningMode::AutoHd).await.unwrap();
    tuner.hd_preset_up().await.unwrap();
    tuner.hd_preset_down().await.unwrap();
    tuner.select_hd_preset(3).await.unwrap();
    tuner.store_hd_preset(12).await.unwrap();
    let fm = TunerFrequency::from_hundredths(8750).unwrap();
    tuner.set_hd_frequency(fm).await.unwrap();
    tuner.set_hd_frequency_multicast(fm, 5).await.unwrap();
    for error in [
        tuner.set_hd_frequency_multicast(fm, 9).await.unwrap_err(),
        tuner.select_hd_multicast(9).await.unwrap_err(),
        tuner.select_hd_preset(57).await.unwrap_err(),
        tuner.store_hd_preset(0).await.unwrap_err(),
    ] {
        assert!(error.is_invalid_input(), "{error:?}");
    }
    wait_until("the commands", || commands_sent(&telnet).len() >= 11).await;
    assert_eq!(
        commands_sent(&telnet),
        [
            "TFHDUP",
            "TFHDDOWN",
            "TFHDMC2",
            "TMHDFM",
            "TMHDAUTOHD",
            "TPHDUP",
            "TPHDDOWN",
            "TPHD03",
            "TPHDMEM12",
            "TFHD008750",
            "TFHD008750MC5",
        ]
    );
}

#[tokio::test]
async fn dirac_slots_are_chosen_by_number_and_by_name() {
    let http = avr_x_receiver_with_info(AVR_X_DIRAC_INFO).await;
    let telnet = FakeTelnet::start().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    let dirac = avr.dirac().unwrap();
    dirac.select_slot(1).await.unwrap();
    dirac.set_filter(DiracFilter::Off).await.unwrap();
    for number in [0, 4] {
        let error = dirac.select_slot(number).await.unwrap_err();
        assert!(matches!(error, Error::DiracSlotOutOfRange(n) if n == number));
        assert!(error.is_invalid_input());
    }
    // Names come from the state.
    let error = dirac.select_slot_by_name("Dirac ART").await.unwrap_err();
    assert!(matches!(error, Error::UnknownDiracSlot(_)), "{error:?}");
    telnet.send(b"OPDLSSL3NAME Dirac ART      \rOPDLSSL3IMP ON\rOPDLSSL3TYP ART\rOPDLS END\r");
    wait_until("the slot", || {
        avr.snapshot()
            .device()
            .dirac_filter_named("dirac art")
            .is_some()
    })
    .await;
    dirac.select_slot_by_name("dirac art").await.unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 3).await;
    assert_eq!(
        commands_sent(&telnet),
        ["PSDIRAC 1", "PSDIRAC OFF", "PSDIRAC 3"]
    );
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

const NET_AUDIO_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/AVR-3312-formNetAudio_StatusXml.xml");
const TUNER_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/Own-formTuner_TunerXml-fm.xml");
const HD_RADIO_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/Own-formTuner_HdXml-playing.xml");

#[tokio::test]
async fn now_playing_is_read_for_the_source_that_plays() {
    static PAGES: [(&str, &[u8]); 3] = [
        ("/goform/formNetAudio_StatusXml.xml", NET_AUDIO_PAGE),
        ("/goform/formTuner_HdXml.xml", HD_RADIO_PAGE),
        ("/goform/formTuner_TunerXml.xml", TUNER_PAGE),
    ];
    let http = avr_x_receiver_with_pages(AVR_X_INFO, &PAGES).await;
    let telnet = FakeTelnet::start().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    let source = |name: &'static str| {
        wait_until("the source", || {
            avr.snapshot().zone(Zone::Main).source.as_deref() == Some(name)
        })
    };

    // A source that does not play anything: no page, no picture.
    telnet.send(b"SICD\r");
    source("CD").await;
    assert_eq!(
        avr.update_now_playing().await.unwrap(),
        PageStatus::Skipped(SkipReason::NoPlayingSource)
    );
    assert_eq!(avr.artwork_url(), None);

    // Network audio.
    telnet.send(b"SINET\r");
    source("NET").await;
    assert_eq!(avr.update_now_playing().await.unwrap(), PageStatus::Read);
    let state = avr.snapshot();
    let now = &state.device().now_playing;
    assert_eq!(
        now.title.as_deref(),
        Some("Moetley Crüe - Girls, girls, girls")
    );
    assert_eq!(now.is_playing(), Some(true));
    let cover = avr.artwork_url().unwrap();
    assert!(cover.contains("/NetAudio/art.asp-jpg?"), "{cover}");
    // The address changes with the title.
    telnet.send(b"NSE1Another song\r");
    wait_until("the title", || {
        avr.snapshot().device().now_playing.title.as_deref() == Some("Another song")
    })
    .await;
    assert_ne!(avr.artwork_url().unwrap(), cover);

    // HD radio.
    telnet.send(b"SIHDRADIO\r");
    source("HDRADIO").await;
    assert_eq!(avr.update_now_playing().await.unwrap(), PageStatus::Read);
    assert_eq!(
        avr.snapshot().device().now_playing.station.as_deref(),
        Some("WXYZ-HD2")
    );
    assert!(
        avr.artwork_url()
            .unwrap()
            .ends_with("/img/album%20art_S.png")
    );

    // The tuner: the band and the frequency, and no title.
    telnet.send(b"SITUNER\r");
    source("TUNER").await;
    assert_eq!(avr.update_now_playing().await.unwrap(), PageStatus::Read);
    let state = avr.snapshot();
    assert!(state.device().now_playing.is_empty());
    assert_eq!(state.device().tuner.band, Some(TunerBand::Fm));
    assert_eq!(state.device().tuner.frequency.unwrap().hundredths(), 8750);

    // Back to a source that plays nothing: what was known is cleared.
    telnet.send(b"SICD\r");
    source("CD").await;
    avr.update_now_playing().await.unwrap();
    assert!(avr.snapshot().device().now_playing.is_empty());
}

#[tokio::test]
async fn an_ipv6_host_works_with_and_without_brackets() {
    let http = avr_x_receiver_on(LOOPBACK_V6).await;
    for host in ["[::1]", "::1"] {
        let telnet = FakeTelnet::start_on(LOOPBACK_V6).await;
        let avr = Avr::builder(host)
            .http_config(common::config())
            .background_pause(Duration::ZERO)
            .probes(&[(ReceiverType::AvrX, http.port)])
            .description_port(http.port)
            .telnet_config(telnet_config(telnet.port))
            .connect()
            .await
            .unwrap();
        assert_eq!(avr.info().host, "::1");
        avr.telnet_connect().await.unwrap();
        telnet.send(b"SINET\r");
        wait_until("the source", || {
            avr.snapshot().zone(Zone::Main).source.as_deref() == Some("NET")
        })
        .await;
        let cover = avr.artwork_url().unwrap();
        let start = format!("http://[::1]:{}/", http.port);
        assert!(cover.starts_with(&start), "{cover}");
    }
}

#[tokio::test]
async fn now_playing_is_requested_for_the_source_that_plays() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let asked = |text: &str| telnet.received().matches(text).count();
    // Nothing plays: nothing is sent.
    telnet.send(b"SICD\r");
    wait_until("the source", || {
        avr.snapshot().zone(Zone::Main).source.as_deref() == Some("CD")
    })
    .await;
    avr.telnet_refresh_now_playing().await.unwrap();
    assert_eq!(asked("NSE\r"), 0);
    telnet.send(b"SINET\r");
    wait_until("the source", || {
        avr.snapshot().zone(Zone::Main).source.as_deref() == Some("NET")
    })
    .await;
    avr.telnet_refresh_now_playing().await.unwrap();
    wait_until("the request", || telnet.received().contains("NSE\r")).await;
    telnet.send(b"SITUNER\r");
    wait_until("the source", || {
        avr.snapshot().zone(Zone::Main).source.as_deref() == Some("TUNER")
    })
    .await;
    avr.telnet_refresh_now_playing().await.unwrap();
    wait_until("the tuner request", || {
        telnet.received().contains("TFANNAME?\r")
    })
    .await;
    // The receiver has no DAB tuner.
    assert!(!telnet.received().contains("DAODR ?\r"));
}

#[tokio::test]
async fn refresh_uses_telnet_when_it_is_open_and_the_page_when_it_is_not() {
    static PAGES: [(&str, &[u8]); 1] = [("/goform/formNetAudio_StatusXml.xml", NET_AUDIO_PAGE)];
    let http = avr_x_receiver_with_pages(AVR_X_INFO, &PAGES).await;
    let telnet = FakeTelnet::start().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .connect()
        .await
        .unwrap();
    // The state needs a source that plays, for both ways.
    avr.telnet_connect().await.unwrap();
    telnet.send(b"SINET\r");
    wait_until("the source", || {
        avr.snapshot().zone(Zone::Main).source.as_deref() == Some("NET")
    })
    .await;
    avr.refresh_now_playing().await.unwrap();
    wait_until("the request", || telnet.received().contains("NSE\r")).await;
    assert!(avr.snapshot().device().now_playing.is_empty());
    // The answer of the receiver fills the state.
    telnet.send(b"NSE0Now Playing\rNSE1Song\r");
    wait_until("the title", || {
        avr.snapshot().device().now_playing.title.as_deref() == Some("Song")
    })
    .await;

    avr.telnet_disconnect().await;
    avr.refresh_now_playing().await.unwrap();
    assert_eq!(
        avr.snapshot().device().now_playing.title.as_deref(),
        Some("Moetley Crüe - Girls, girls, girls")
    );
}

#[tokio::test]
async fn refresh_reads_http_and_asks_on_telnet_when_it_is_open() {
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .connect()
        .await
        .unwrap();
    let appcommand = || {
        http.requests()
            .iter()
            .filter(|seen| seen.path == "/goform/AppCommand.xml")
            .count()
    };
    // Without telnet only HTTP is read, and the zone names come with it.
    let before = appcommand();
    avr.refresh().await.unwrap();
    assert!(appcommand() > before);
    assert!(avr.snapshot().zone(Zone::Main).name.is_some());
    assert_eq!(telnet.received(), "");

    // With telnet the queries are sent again too.
    avr.telnet_connect().await.unwrap();
    wait_until("the first query", || telnet.received().contains("SI?\r")).await;
    let asked = telnet.received().matches("SI?\r").count();
    let before = appcommand();
    avr.refresh().await.unwrap();
    assert_eq!(appcommand(), before + 1);
    // Only the zone names are read over HTTP, the rest comes from telnet.
    let last = http.requests().pop().unwrap();
    let body = String::from_utf8_lossy(&last.body);
    assert!(body.contains("GetZoneName"), "{body}");
    assert!(!body.contains("GetAllZoneVolume"), "{body}");
    // The default groups of this test have the device settings.
    assert!(!body.contains("GetDimmer"), "{body}");
    assert_eq!(
        avr.snapshot().zone(Zone::Zone2).name.as_deref(),
        Some("Kitchen")
    );
    wait_until("the queries again", || {
        telnet.received().matches("SI?\r").count() > asked
    })
    .await;
}

#[tokio::test]
async fn refresh_reads_the_device_settings_that_telnet_does_not_fetch() {
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver().await;
    let config = TelnetConfig {
        query_groups: QueryGroup::START_ORDER,
        ..telnet_config(telnet.port)
    };
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(config)
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    status_is(&avr, TelnetStatus::Connected).await;
    assert_eq!(avr.snapshot().device().dimmer, None);

    avr.refresh().await.unwrap();
    let last = http.requests().pop().unwrap();
    let body = String::from_utf8_lossy(&last.body);
    for command in ["GetZoneName", "GetAutoStandby", "GetDimmer", "GetECO"] {
        assert!(body.contains(command), "{command}: {body}");
    }
    let state = avr.snapshot();
    assert_eq!(state.device().dimmer, Some(DimmerMode::Dim));
    assert_eq!(state.device().eco_mode, Some(EcoMode::On));
    assert_eq!(
        state.zone(Zone::Main).auto_standby,
        Some(AutoStandby::Minutes60)
    );
    assert_eq!(state.zone(Zone::Zone2).name.as_deref(), Some("Kitchen"));
}

#[tokio::test]
async fn the_status_is_reset_when_the_task_stops_before_it_runs() {
    // This test runs on one thread, so the task does not run before the drop.
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver().await;
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .connect()
        .await
        .unwrap();
    let status = avr.telnet_status();
    avr.telnet_connect().await.unwrap();
    assert_eq!(*status.borrow(), TelnetStatus::Connected);
    drop(avr);
    wait_until("the status", || {
        *status.borrow() == TelnetStatus::Disconnected
    })
    .await;
}

#[tokio::test]
#[cfg_attr(
    windows,
    ignore = "a Windows loopback socket takes 16 MiB without blocking"
)]
async fn a_stop_does_not_wait_for_a_write_that_is_blocked() {
    // A port that takes the connection and never reads, so a large write blocks.
    let socket = tokio::net::TcpSocket::new_v4().unwrap();
    // Accepted sockets keep the small buffer.
    socket.set_recv_buffer_size(4096).unwrap();
    socket.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let listener = socket.listen(1).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (written, first_bytes) = tokio::sync::oneshot::channel();
    let holder = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.unwrap();
        // The first bytes show that the write has started.
        socket.readable().await.unwrap();
        let _ = written.send(());
        std::future::pending::<()>().await;
    });
    let http = avr_x_receiver().await;
    let config = TelnetConfig {
        write_timeout: Duration::from_secs(10),
        query_groups: &[],
        ..telnet_config(port)
    };
    let avr = std::sync::Arc::new(
        builder(&http, ReceiverType::AvrX)
            .telnet_config(config)
            .connect()
            .await
            .unwrap(),
    );
    avr.telnet_connect().await.unwrap();
    // More than the buffers of both sockets hold.
    let huge = "A".repeat(16 * 1024 * 1024);
    let sender = tokio::spawn({
        let avr = avr.clone();
        async move { avr.telnet_send(Zone::Main, Command::Raw(&huge)).await }
    });
    timeout(Duration::from_secs(5), first_bytes)
        .await
        .expect("the write did not start")
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!sender.is_finished());

    let stopped = std::time::Instant::now();
    avr.telnet_disconnect().await;
    assert!(stopped.elapsed() < Duration::from_secs(2));
    // Part of the line was written.
    let error = sender.await.unwrap().unwrap_err();
    assert!(error.is_network() && !error.is_not_sent(), "{error:?}");
    holder.abort();
}

const VIDEO_PATH: &str = "/ajax/info/get_config?type=3";
const CHANNELS_PATH: &str = "/ajax/info/get_config?type=2";
const CHANNELS_PAGE: &[u8] =
    include_bytes!("../../denon-marantz-proto/tests/data/xml/AVC-A10H-web-audio-channels.xml");

#[tokio::test]
async fn repeated_group_requests_do_not_grow_the_queue() {
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver().await;
    let config = TelnetConfig {
        query_groups: &[],
        query_interval: Duration::from_millis(60),
        ..telnet_config(telnet.port)
    };
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(config)
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    // The queue is slower than the requests, so these would pile up.
    for _ in 0..5 {
        avr.telnet_refresh_group(QueryGroup::Signal).await.unwrap();
    }
    wait_until("the last query of the group", || {
        sent(&telnet).contains(&"SSINFSIGECI ?".to_owned())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    let lines = sent(&telnet);
    let count = |query: &str| lines.iter().filter(|line| line.as_str() == query).count();
    assert_eq!(count("SYSDA ?"), 1, "{lines:?}");
    assert_eq!(count("SSINFSIGRES ?"), 1, "{lines:?}");
}

#[tokio::test]
async fn a_signal_change_asks_for_the_colorimetry_again() {
    let telnet = FakeTelnet::start().await;
    let http = avr_x_receiver().await;
    let config = TelnetConfig {
        query_groups: &[],
        ..telnet_config(telnet.port)
    };
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(config)
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    let bt2020 = || avr.snapshot().device().signal.bt2020.input;
    let asked = || {
        sent(&telnet)
            .iter()
            .filter(|line| *line == "SSINFSIGECI ?")
            .count()
    };
    telnet.send(b"SSINFSIGCOS 11\rSSINFSIGECI 55\r");
    wait_until("BT.2020", || bt2020() == Some(true)).await;
    assert_eq!(asked(), 0);

    // A new resolution makes BT.2020 not known, so it is asked once.
    telnet.send(b"SSINFSIGRES I1080p50\r");
    wait_until("the colorimetry query", || asked() == 1).await;
    assert_eq!(bt2020(), None);
    // The answer repeats the last value, and must still count.
    telnet.send(b"SSINFSIGECI 55\r");
    wait_until("BT.2020 again", || bt2020() == Some(true)).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(asked(), 1);
}

/// An `Avr` with telnet that asks nothing after the connect, and the given
/// wait after a signal hint.
async fn connected_with_signal_wait(telnet: &FakeTelnet, wait: Duration) -> Avr {
    let http = avr_x_receiver().await;
    let config = TelnetConfig {
        query_groups: &[],
        signal_wait_min: wait,
        signal_wait_max: wait,
        ..telnet_config(telnet.port)
    };
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(config)
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    avr
}

fn count_sent(telnet: &FakeTelnet, query: &str) -> usize {
    sent(telnet).iter().filter(|line| *line == query).count()
}

#[tokio::test]
async fn a_signal_hint_asks_for_the_video_signal_once() {
    let telnet = FakeTelnet::start().await;
    let _avr = connected_with_signal_wait(&telnet, Duration::from_millis(100)).await;
    // The burst of a signal change, with values that did not change.
    telnet.send(b"MSDOLBY ATMOS\rPSLFE 00\rSSINFAISSIG 25\rSYSDA Dolby Atmos\r");
    telnet.send(b"MSDOLBY ATMOS\rSSINFAISSIG 25\r");
    wait_until("the video queries", || {
        count_sent(&telnet, "SSINFSIGRES ?") == 1 && count_sent(&telnet, "SSINFSIGHDR ?") == 1
    })
    .await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(count_sent(&telnet, "SSINFSIGRES ?"), 1);
}

#[tokio::test]
async fn a_new_sample_rate_is_a_signal_hint() {
    let telnet = FakeTelnet::start().await;
    let _avr = connected_with_signal_wait(&telnet, Duration::from_millis(100)).await;
    telnet.send(b"SSINFAISFSV 48K\r");
    telnet.send(b"SSINFAISFSV 48K\r");
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(count_sent(&telnet, "SSINFSIGRES ?"), 0);
    telnet.send(b"SSINFAISFSV 441\r");
    wait_until("the video queries", || {
        count_sent(&telnet, "SSINFSIGRES ?") == 1
    })
    .await;
}

#[tokio::test]
async fn a_signal_read_without_values_asks_once_more() {
    let telnet = FakeTelnet::start().await;
    let _avr = connected_with_signal_wait(&telnet, Duration::from_millis(100)).await;
    telnet.send(b"MSDOLBY ATMOS\r");
    wait_until("the video queries", || {
        count_sent(&telnet, "SSINFSIGRES ?") == 1
    })
    .await;
    // Not ready yet.
    telnet.send(b"SSINFSIGRES I--- \rSSINFSIGRES O--- \r");
    wait_until("the second read", || {
        count_sent(&telnet, "SSINFSIGRES ?") == 2
    })
    .await;
    telnet.send(b"SSINFSIGRES I--- \rSSINFSIGRES O--- \r");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(count_sent(&telnet, "SSINFSIGRES ?"), 2);
}

#[tokio::test]
async fn a_video_signal_line_cancels_the_signal_hint() {
    let telnet = FakeTelnet::start().await;
    let _avr = connected_with_signal_wait(&telnet, Duration::from_millis(300)).await;
    telnet.send(b"MSSTEREO\r");
    tokio::time::sleep(Duration::from_millis(50)).await;
    // Another client asked.
    telnet.send(b"SSINFSIGRES I4K24\r");
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        count_sent(&telnet, "SSINFSIGRES ?"),
        0,
        "{:?}",
        sent(&telnet)
    );
}

#[tokio::test]
async fn the_answer_to_an_own_sound_mode_query_is_not_a_signal_hint() {
    let telnet = FakeTelnet::start().await;
    let avr = connected_with_signal_wait(&telnet, Duration::from_millis(100)).await;
    avr.telnet_ask(&[Ask::Raw("MS?")]).await.unwrap();
    wait_until("the query", || count_sent(&telnet, "MS?") == 1).await;
    telnet.send(b"MSSTEREO\r");
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(
        count_sent(&telnet, "SSINFSIGRES ?"),
        0,
        "{:?}",
        sent(&telnet)
    );
}

#[tokio::test]
async fn a_page_does_not_hide_the_next_telnet_event_with_the_old_value() {
    let telnet = FakeTelnet::start().await;
    let web = common::Fake::start(|seen| match seen.path.as_str() {
        VIDEO_PATH => Reply::Body(
            b"<Video><Resolution><Input>1080p60</Input><Output>1080p60</Output></Resolution></Video>"
                .to_vec(),
        ),
        CHANNELS_PATH => Reply::Body(CHANNELS_PAGE.to_vec()),
        _ => Reply::Status(404),
    })
    .await;
    let http = avr_x_receiver().await;
    let avr = std::sync::Arc::new(
        builder(&http, ReceiverType::AvrX)
            .web_interface_port(web.port)
            .web_interface_config(common::config())
            .telnet_config(telnet_config(telnet.port))
            .connect()
            .await
            .unwrap(),
    );
    let video = |avr: &Avr| avr.snapshot().device().signal.video.input.clone();
    avr.telnet_connect().await.unwrap();
    telnet.send(b"SSINFSIGRES I4K50\r");
    wait_until("the telnet value", || {
        video(&avr).as_deref() == Some("4K50")
    })
    .await;

    // The start reads the page, and telnet is already open, so telnet is not asked.
    let mut started = avr.start().await.unwrap();
    let report = started.basic_synced().await;
    assert!(matches!(
        report.telnet,
        denon_marantz::TelnetStart::AlreadyRunning
    ));
    assert_eq!(video(&avr).as_deref(), Some("1080p60"));

    // The old value is a real change now.
    telnet.send(b"SSINFSIGRES I4K50\r");
    wait_until("the old value again", || {
        video(&avr).as_deref() == Some("4K50")
    })
    .await;
}

#[tokio::test]
async fn a_sound_mode_of_the_list_is_selected_with_its_code() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    telnet.send(b"OPSMLALL MOV020Dolby Surround\rOPSMLALL MOV030Something New\rOPSMLALL END\r");
    wait_until("the list", || {
        avr.snapshot().device().sound_modes.as_slice().len() == 2
    })
    .await;
    let state = avr.snapshot();
    let modes = state.device().sound_modes.as_slice();
    let main = avr.zone(Zone::Main).unwrap();
    main.select_sound_mode_entry(&modes[0]).await.unwrap();
    // A mode without a known code sends nothing.
    let error = main.select_sound_mode_entry(&modes[1]).await.unwrap_err();
    assert!(matches!(error, Error::UnknownSoundMode(_)), "{error:?}");
    assert!(error.is_invalid_input());
    // Only the main zone has a sound mode.
    let error = avr
        .zone(Zone::Zone2)
        .unwrap()
        .select_sound_mode_entry(&modes[0])
        .await
        .unwrap_err();
    assert!(
        matches!(error, Error::NoSoundMode(Zone::Zone2)),
        "{error:?}"
    );
    wait_until("the command", || !commands_sent(&telnet).is_empty()).await;
    assert_eq!(commands_sent(&telnet), ["MSDOLBY DIGITAL"]);
}

/// An `Avr` with telnet that uses the stored values `info`, changed by `edit`.
async fn connected_known(
    telnet: &FakeTelnet,
    edit: impl FnOnce(&mut denon_marantz::Info),
) -> (Avr, common::Fake) {
    let http = avr_x_receiver().await;
    let mut info = builder(&http, ReceiverType::AvrX)
        .connect()
        .await
        .unwrap()
        .info()
        .clone();
    edit(&mut info);
    let avr = builder(&http, ReceiverType::AvrX)
        .telnet_config(telnet_config(telnet.port))
        .known(info)
        .connect()
        .await
        .unwrap();
    avr.telnet_connect().await.unwrap();
    (avr, http)
}

#[tokio::test]
async fn setters_refuse_values_outside_the_ranges_of_the_receiver() {
    let telnet = FakeTelnet::start().await;
    let range = |min_tenths, max_tenths| {
        Some(ValueRange {
            min_tenths,
            max_tenths,
            step_tenths: Some(10),
        })
    };
    let (avr, _http) = connected_known(&telnet, |info| {
        info.ranges.auro_matic_strength = range(0, 150);
        info.ranges.lfe = range(-50, 0);
        info.ranges.zones[1].bass = range(-60, 60);
        info.ranges.zones[1].treble = Some(ValueRange {
            min_tenths: -140,
            max_tenths: 140,
            step_tenths: Some(20),
        });
    })
    .await;
    let sound = avr.sound();
    let zone2 = avr.zone(Zone::Zone2).unwrap();
    let errors = [
        sound.set_auro_matic_strength(16).await.unwrap_err(),
        sound.set_lfe_level(6).await.unwrap_err(),
        zone2
            .set_bass(ToneLevel::from_db(7).unwrap())
            .await
            .unwrap_err(),
        // Between two steps.
        zone2
            .set_treble(ToneLevel::from_db(13).unwrap())
            .await
            .unwrap_err(),
    ];
    for error in errors {
        assert!(matches!(error, Error::NumberOutOfRange { .. }), "{error:?}");
    }
    sound.set_auro_matic_strength(15).await.unwrap();
    sound.set_lfe_level(5).await.unwrap();
    zone2
        .set_bass(ToneLevel::from_db(-6).unwrap())
        .await
        .unwrap();
    zone2
        .set_treble(ToneLevel::from_db(14).unwrap())
        .await
        .unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 4).await;
    assert_eq!(
        commands_sent(&telnet),
        ["PSAUROST 15", "PSLFE 05", "Z2PSBAS 44", "Z2PSTRE 64"]
    );
}

#[tokio::test]
async fn queries_of_features_that_the_receiver_does_not_list_are_not_sent() {
    let telnet = FakeTelnet::start().await;
    let (_avr, _http) = connected_known(&telnet, |info| {
        info.features = Some(DeviceFeatures::default().with(DeviceFeature::Imax));
    })
    .await;
    // The last query of the default order.
    wait_until("the queries", || telnet.received().contains("MNPRV?\r")).await;
    let received = telnet.received();
    for asked in ["MV?\r", "PSIMAX ?\r", "CV?\r", "SSQS ?\r"] {
        assert!(received.contains(asked), "{asked:?} in {received:?}");
    }
    for skipped in [
        "PSIMAXSWM ?\r",
        "PSAUROST ?\r",
        "PV?\r",
        "Z3?\r",
        "SSVCTZ3S ?\r",
    ] {
        assert!(!received.contains(skipped), "{skipped:?} in {received:?}");
    }
}

#[tokio::test]
async fn balance_lfe_input_tactile_bluetooth_and_dab_setters_send_their_text() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected(telnet_config(telnet.port)).await;
    let sound = avr.sound();
    sound.set_balance(-2).await.unwrap();
    sound.set_balance(0).await.unwrap();
    sound.balance_right().await.unwrap();
    sound.request_balance().await.unwrap();
    sound.request_lfe_external_input().await.unwrap();
    sound.set_lfe_external_input(5).await.unwrap();
    sound
        .set_tactile_channel_level(ChannelLevel::from_half_db(-24).unwrap())
        .await
        .unwrap();
    sound.set_tactile_channel(false).await.unwrap();
    avr.device().set_bluetooth_level(85).await.unwrap();
    let tuner = avr.tuner();
    tuner.preset_off().await.unwrap();
    tuner.set_dab_drc(DabDrc::Off).await.unwrap();
    tuner
        .set_dab_station_order(DabStationOrder::Alphanumeric)
        .await
        .unwrap();
    for error in [
        sound.set_balance(13).await.unwrap_err(),
        sound.set_lfe_external_input(4).await.unwrap_err(),
        avr.device().set_bluetooth_level(91).await.unwrap_err(),
    ] {
        assert!(matches!(error, Error::NumberOutOfRange { .. }), "{error:?}");
    }
    wait_until("the commands", || commands_sent(&telnet).len() >= 10).await;
    // The state queries do not ask for these.
    assert!(telnet.received().contains("BL?\r"));
    assert!(telnet.received().contains("PSLFL ?\r"));
    assert_eq!(
        commands_sent(&telnet),
        [
            "BLL02",
            "BL0",
            "BLRIGHT",
            "PSLFL 05",
            "CVTTR 38",
            "CVTTR OFF",
            "BTLEV 85",
            "TPANOFF",
            "DADRC OFF",
            "DAODR ALP"
        ]
    );
}

#[tokio::test]
async fn a_dab_tuner_is_asked_for_its_status_with_the_tuner() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected_known(&telnet, |info| {
        info.tuner = TunerInfo::Present(TunerBands::default().with(TunerBand::Dab));
    })
    .await;
    telnet.send(b"SITUNER\r");
    wait_until("the DAB queries", || {
        telnet.received().contains("DAODR ?\r")
    })
    .await;
    // `SYSDA ?` ends with the same text.
    assert!(telnet.received().contains("\rDA ?\r"));
    wait_until("the source", || {
        avr.snapshot().zone(Zone::Main).source.as_deref() == Some("TUNER")
    })
    .await;
    avr.telnet_refresh_now_playing().await.unwrap();
    wait_until("the DAB queries again", || {
        telnet.received().matches("DAODR ?\r").count() == 2
    })
    .await;
}

#[tokio::test]
async fn a_group_refresh_skips_the_queries_of_features_that_are_not_listed() {
    let telnet = FakeTelnet::start().await;
    let (avr, _http) = connected_known(&telnet, |info| {
        info.features = Some(DeviceFeatures::default().with(DeviceFeature::Imax));
    })
    .await;
    wait_until("the start queries", || {
        telnet.received().contains("MNPRV?\r")
    })
    .await;
    avr.telnet_refresh_group(QueryGroup::Sound).await.unwrap();
    wait_until("the group", || {
        telnet.received().matches("PSIMAX ?\r").count() == 2
    })
    .await;
    assert!(!telnet.received().contains("PSIMAXSWM ?\r"));
}

#[tokio::test]
async fn zone_levels_refuse_values_outside_the_ranges_of_the_receiver() {
    let telnet = FakeTelnet::start().await;
    let range = Some(ValueRange {
        min_tenths: -60,
        max_tenths: 60,
        step_tenths: Some(10),
    });
    let (avr, _http) = connected_known(&telnet, |info| {
        info.ranges.zones[1].left_level = range;
        info.ranges.zones[1].right_level = range;
    })
    .await;
    let zone2 = avr.zone(Zone::Zone2).unwrap();
    for error in [
        zone2.set_left_level(7).await.unwrap_err(),
        zone2.set_right_level(-7).await.unwrap_err(),
    ] {
        assert!(matches!(error, Error::NumberOutOfRange { .. }), "{error:?}");
    }
    zone2.set_left_level(6).await.unwrap();
    zone2.set_right_level(-6).await.unwrap();
    wait_until("the commands", || commands_sent(&telnet).len() >= 2).await;
    assert_eq!(commands_sent(&telnet), ["Z2CVFL 56", "Z2CVFR 44"]);
}
