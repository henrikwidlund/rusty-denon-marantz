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
    SettingsGroup, SoundModeGroup, State, SystemSettings, VideoSignalPage, Volume, Zone, source,
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
    assert!(
        info.count_total < 50,
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
