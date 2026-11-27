//! Checks allocations on hot paths.
//!
//! Telnet decoding, command encoding and the tables do not allocate. XML
//! parsing allocates a little for each document. The counter is per thread.

use std::fs;
use std::hint::black_box;
use std::path::PathBuf;

use allocation_counter::measure;
use denon_marantz_proto::appcommand::{self as ac, Cmd, Param, Response, write_body};
use denon_marantz_proto::telnet::{Command, DuplicateFilter, Event, LineDecoder, Message};
use denon_marantz_proto::{
    AudioChannelsPage, Brand, ChannelLevel, DeviceInfo, DimmerMode, GroupAvailability, NowPlaying,
    PlayingKind, SettingsGroup, SoundModeGroup, State, SystemSettings, VideoSignalPage, Volume,
    Zone, source,
};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/xml")
        .join(name);
    fs::read(path).unwrap()
}

/// Runs `work` and fails if it allocates.
fn assert_no_allocation(what: &str, work: impl FnOnce()) {
    let info = measure(work);
    assert_eq!(
        info.count_total, 0,
        "{what} allocated {} times",
        info.count_total
    );
}

#[test]
fn telnet_decoding_does_not_allocate() {
    let mut decoder = LineDecoder::new();
    // Lines inside one read are not copied.
    assert_no_allocation("decode complete lines", || {
        decoder.decode(b"PWON\rMV435\rZMON\rZ2MUON\rSIGAME\r", |line| {
            black_box(Message::parse(line));
        });
    });

    // A split line is copied. After the first one the buffer is reused.
    decoder.decode(b"MS", |_| {});
    decoder.decode(b"STEREO\r", |_| {});
    assert_no_allocation("decode split lines", || {
        decoder.decode(b"MS", |_| {});
        decoder.decode(b"STEREO\r", |line| {
            black_box(line);
        });
    });

    assert_no_allocation("parse messages", || {
        for line in [
            "PWON",
            "MV435",
            "Z2ON",
            "Z3CD",
            "Z243",
            "Z2MUON",
            "NSE1Hello",
            "SYSDV END",
            "XYZ",
        ] {
            black_box(Message::parse(black_box(line)));
        }
        black_box(Event::split_prefix(black_box("STBY2H")));
    });

    let mut filter = DuplicateFilter::new();
    filter.accept("INFSIGRES I 4K");
    assert_no_allocation("duplicate filter", || {
        black_box(filter.accept("INFSIGRES I 4K"));
        black_box(filter.accept("INFSIGRES I 8K"));
        black_box(filter.accept("ON"));
    });
}

#[test]
fn encoding_and_tables_do_not_allocate() {
    let mut line = String::with_capacity(64);
    let volume = Volume::from_db_rounded(-36.5).unwrap();
    assert_no_allocation("encode commands", || {
        for zone in Zone::ALL {
            for command in [
                Command::PowerOn,
                Command::SetVolume(volume),
                Command::SelectSource("GAME"),
                Command::Mute(true),
            ] {
                line.clear();
                command.write_line(Brand::Denon, zone, &mut line).unwrap();
                black_box(&line);
            }
        }
        black_box(Volume::from_telnet(black_box("435")));
    });

    assert_no_allocation("tables", || {
        black_box(SoundModeGroup::from_raw(black_box(
            "dolby audio - dd+   + dsur",
        )));
        black_box(SoundModeGroup::guess_from_raw(black_box("new cinema mode")));
        black_box(DimmerMode::from_appcommand(black_box("3")));
        black_box(ChannelLevel::from_code(black_box("495")));
        black_box(source::xml_to_telnet_source(black_box("Blu-ray")));
        black_box(source::is_playing_source(black_box("NET")));
    });

    let params = [Param {
        name: "multeq",
        text: "1",
    }];
    let commands = [Cmd::get("GetDimmer"), Cmd::set_audyssey(&params)];
    let mut body = Vec::with_capacity(1024);
    assert_no_allocation("write request body", || {
        write_body(&commands, &mut body).unwrap();
        black_box(&body);
    });
}

/// XML parsing allocates a little for each reader. The limits are far above
/// today's numbers. They catch a change that allocates for each element.
#[test]
fn xml_parsing_allocates_little() {
    let update = fixture("AVC-X3700H-AppCommand-update-8080.xml");
    let items = 11;
    let definitions = [
        ac::GET_ALL_ZONE_POWER_STATUS,
        ac::GET_AUTO_STANDBY,
        ac::GET_DIMMER,
        ac::GET_ECO,
        ac::GET_ALL_ZONE_SOURCE,
    ];
    let info = measure(|| {
        let response = Response::parse(&update, items).unwrap();
        for (definition, item) in definitions.iter().zip(response.items()) {
            definition.extract(&item, Zone::Main, &mut |field, value| {
                black_box((field, value));
            });
        }
    });
    assert!(
        info.count_total < 200,
        "answer used {} allocations",
        info.count_total
    );

    let device_info = fixture("AVC-X3700H-Deviceinfo-8080.xml");
    let info = measure(|| {
        black_box(
            DeviceInfo::parse(&device_info)
                .map(|info| info.is_avr_x())
                .ok(),
        );
    });
    // Each search of the whole file costs about 10, the walk over the listed
    // queries about 100, and the features and ranges about 120: each element
    // that is read inside costs a few. The file is read once at connect.
    assert!(
        info.count_total < 320,
        "device info used {} allocations",
        info.count_total
    );
}

/// The pages of the web interface and the status pages. The limits are about
/// twice today's numbers. Applying a page that did not change does not
/// allocate: the state keeps its buffers.
#[test]
fn web_interface_pages_allocate_little() {
    let video = fixture("AVC-A10H-web-video-signal.xml");
    let audio = fixture("AVC-A10H-web-audio-channels.xml");
    let system = fixture("AVC-A10H-web-system-settings.xml");
    let surround = fixture("AVC-A10H-web-details-surround.xml");
    let net = fixture("AVR-3312-formNetAudio_StatusXml.xml");
    let radio = fixture("Own-formTuner_HdXml-playing.xml");

    let allocations = |work: &mut dyn FnMut()| measure(work).count_total;
    let cases = [
        (
            "video signal page",
            120,
            allocations(&mut || {
                black_box(VideoSignalPage::parse(&video).unwrap());
            }),
        ),
        (
            "channels page",
            60,
            allocations(&mut || {
                black_box(AudioChannelsPage::parse(&audio).unwrap());
            }),
        ),
        (
            "system settings page",
            70,
            allocations(&mut || {
                black_box(SystemSettings::parse(&system).unwrap());
            }),
        ),
        (
            "detail page",
            90,
            allocations(&mut || {
                black_box(
                    GroupAvailability::parse(SettingsGroup::SurroundParameters, &surround).unwrap(),
                );
            }),
        ),
        (
            "network audio page",
            60,
            allocations(&mut || {
                black_box(NowPlaying::parse_net_audio(&net).unwrap());
            }),
        ),
        (
            "HD radio page",
            90,
            allocations(&mut || {
                black_box(NowPlaying::parse_hd_radio(&radio).unwrap());
            }),
        ),
    ];
    for (what, limit, count) in cases {
        assert!(count < limit, "{what} used {count} allocations");
    }

    let page = VideoSignalPage::parse(&video).unwrap();
    let mut state = State::new(false);
    state.apply_video_signal_page(&page);
    assert_no_allocation("apply the same video page", || {
        black_box(state.apply_video_signal_page(&page));
    });
}

/// The lines that the receiver sends about what plays and about the signal.
const PLAYING_AND_SIGNAL_LINES: [&str; 22] = [
    "NSE0Now Playing",
    "NSE1\u{1}Song title",
    "NSE2Artist",
    "NSE4Album",
    "HDARTISTBand",
    "HDTITLESong",
    "HDALBUMRecord",
    "HDST NAMEWXYZ",
    "SSINFSIGRES I4K50",
    "SSINFSIGHDR I HDR10",
    "SSINFSIGFRL O---",
    "SSINFSIGPIX 11",
    "SSINFSIGCOS 22",
    "SSINFSIGECI 00",
    "SSINFAISFSV 48K",
    "SYSDA PCM",
    "SYSMI Stereo",
    "SYSDVIN 4K50",
    "SYSDVOUT 4K50",
    "SYHDMIDIAGMAXRES NO",
    "OPINFINS 22000000",
    "OPINFASP 22200000",
];

/// A value that did not change does not allocate, and the first value
/// allocates only for its own text.
#[test]
fn unchanged_now_playing_and_signal_events_do_not_allocate() {
    let mut state = State::new(false);
    for line in PLAYING_AND_SIGNAL_LINES {
        state.apply_message(&Message::parse(line).unwrap());
    }
    assert_no_allocation("events with the same values", || {
        for line in PLAYING_AND_SIGNAL_LINES {
            if let Some(message) = Message::parse(black_box(line)) {
                black_box(state.apply_message(&message));
            }
        }
    });
}

#[test]
fn changed_now_playing_text_reuses_the_buffer() {
    let mut state = State::new(false);
    state.apply_message(&Message::parse("NSE1A long first title").unwrap());
    let longer = Message::parse("NSE1Short").unwrap();
    // A shorter text fits in the buffer that is there.
    assert_no_allocation("a changed title", || {
        black_box(state.apply_message(&longer));
    });
}

#[test]
fn the_other_pages_allocate_little() {
    use denon_marantz_proto::{BassSettings, InputAssignment, SourceNames, SpeakerTable};
    let tuner = fixture("Own-formTuner_TunerXml-fm.xml");
    let bass = fixture("AVC-A10H-web-speakers-bass.xml");
    let assign = fixture("AVC-A10H-web-inputs-assign.xml");
    let names = fixture("AVC-A10H-web-inputs-names.xml");
    let distances = fixture("AVC-A10H-web-speakers-distances.xml");
    let count = |work: &mut dyn FnMut()| measure(work).count_total;
    let cases = [
        (
            "tuner page",
            50,
            count(&mut || {
                black_box(denon_marantz_proto::TunerPage::parse(&tuner).unwrap());
            }),
        ),
        (
            "bass page",
            100,
            count(&mut || {
                black_box(BassSettings::parse(&bass).unwrap());
            }),
        ),
        (
            "input assignment page",
            450,
            count(&mut || {
                black_box(InputAssignment::parse(&assign).unwrap());
            }),
        ),
        (
            "source names page",
            130,
            count(&mut || {
                black_box(SourceNames::parse(&names).unwrap());
            }),
        ),
        (
            "distances page",
            70,
            count(&mut || {
                black_box(SpeakerTable::parse_distances(&distances).unwrap());
            }),
        ),
    ];
    for (what, limit, count) in cases {
        assert!(count < limit, "{what} used {count} allocations");
    }
}

/// The pages of the video, general and speaker menus. The limits are about
/// one and a half times today's numbers, so a large rise is caught.
#[test]
fn menu_pages_allocate_little() {
    use denon_marantz_proto::{
        GeneralPage, Playback2Ch, SourceLevels, Trigger, VideoMenuPage, XlrAssign,
    };
    let count = |work: &mut dyn FnMut()| measure(work).count_total;
    let hdcp = fixture("AVC-A10H-web-video-hdcp.xml");
    let formats = fixture("AVC-A10H-web-video-4k8k-per-input.xml");
    let osd = fixture("AVC-A10H-web-video-osd.xml");
    let trigger = fixture("AVC-A10H-web-general-trigger-a.xml");
    let playback = fixture("AVC-A10H-web-speakers-playback-2ch.xml");
    let xlr = fixture("AVC-A10H-web-speakers-xlr-assign.xml");
    let levels = fixture("AVC-A10H-web-inputs-source-levels.xml");
    let cases = [
        (
            "HDCP page",
            120,
            count(&mut || {
                black_box(VideoMenuPage::Hdcp.parse(&hdcp).unwrap());
            }),
        ),
        (
            "signal format page",
            135,
            count(&mut || {
                black_box(VideoMenuPage::SignalFormat.parse(&formats).unwrap());
            }),
        ),
        (
            "on-screen display page",
            32,
            count(&mut || {
                black_box(VideoMenuPage::OnScreenDisplay.parse(&osd).unwrap());
            }),
        ),
        (
            "trigger page",
            70,
            count(&mut || {
                black_box(GeneralPage::Trigger(Trigger::One).parse(&trigger).unwrap());
            }),
        ),
        (
            "playback page",
            65,
            count(&mut || {
                black_box(Playback2Ch::parse(&playback).unwrap());
            }),
        ),
        (
            "XLR page",
            55,
            count(&mut || {
                black_box(XlrAssign::parse(&xlr).unwrap());
            }),
        ),
        (
            "source levels page",
            20,
            count(&mut || {
                black_box(SourceLevels::parse(&levels).unwrap());
            }),
        ),
    ];
    for (what, limit, count) in cases {
        assert!(count < limit, "{what} used {count} allocations");
    }
}

#[test]
fn finding_the_entry_of_a_sound_mode_does_not_allocate() {
    let mut state = State::new(false);
    for line in [
        "OPSMLALL MOV011Stereo",
        "OPSMLALL MOV020Multi Ch In",
        "OPSMLALL MOV030Multi Ch Stereo",
        "OPSMLALL END",
    ] {
        state.apply_message(&Message::parse(line).unwrap());
    }
    let list = &state.device().sound_modes;
    assert_eq!(list.as_slice().len(), 3);
    assert_no_allocation("sound mode entries", || {
        black_box(list.entry_for(black_box("MULTI CH IN")));
        black_box(list.entry_for(black_box("MCH STEREO")));
        black_box(list.as_slice()[1].code());
    });
}

#[test]
fn main_playing_kind_does_not_allocate() {
    let mut state = State::new(false);
    state.apply_renamed_source("CD", Some("Disc"));
    state.apply_renamed_source("HDRADIO", Some("Car Radio"));
    state.apply_field(Zone::Main, ac::Field::InputFunc, Some("Car Radio"));
    // The name that the user gave is found in the source list.
    assert_eq!(state.main_playing_kind(), Some(PlayingKind::HdRadio));
    assert_no_allocation("main playing kind", || {
        black_box(black_box(&state).main_playing_kind());
    });
    // A known name with no change looks at the kind and does not allocate.
    // Other `SS` events look at it too.
    let messages = ["SSFUNHDRADIO Car Radio", "SSSODHDRADIO USE", "SSSMG MOV"]
        .map(|line| Message::parse(line).unwrap());
    state.apply_message(&messages[1]);
    state.apply_message(&messages[2]);
    assert_no_allocation("source names with no change", || {
        black_box(state.apply_renamed_source(black_box("HDRADIO"), Some("Car Radio")));
        for message in &messages {
            black_box(state.apply_message(black_box(message)));
        }
    });
    // A telnet name gives the kind without the source list.
    state.apply_message(&Message::parse("SINET").unwrap());
    assert_eq!(state.main_playing_kind(), Some(PlayingKind::NetAudio));
    assert_no_allocation("main playing kind of a telnet name", || {
        black_box(black_box(&state).main_playing_kind());
    });
}
