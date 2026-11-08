//! Speed of the XML answers, the status pages and the command lines.

use std::fs;
use std::hint::black_box;
use std::path::PathBuf;

use criterion::{Criterion, criterion_group, criterion_main};
use denon_marantz_proto::appcommand::{
    GET_ALL_ZONE_POWER_STATUS, GET_ALL_ZONE_SOURCE, GET_AUTO_STANDBY, GET_DIMMER, GET_ECO, Response,
};
use denon_marantz_proto::status_page::{Page, extract};
use denon_marantz_proto::telnet::Command;
use denon_marantz_proto::{Brand, DeviceInfo, State, Volume, Zone};

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data/xml")
        .join(name);
    fs::read(path).unwrap()
}

fn appcommand(c: &mut Criterion) {
    let update = fixture("AVC-X3700H-AppCommand-update-8080.xml");
    let definitions = [
        GET_ALL_ZONE_POWER_STATUS,
        GET_AUTO_STANDBY,
        GET_DIMMER,
        GET_ECO,
        GET_ALL_ZONE_SOURCE,
    ];
    let items = 11;
    let mut group = c.benchmark_group("xml");

    group.bench_function("appcommand_parse", |b| {
        b.iter(|| black_box(Response::parse(black_box(&update), items).unwrap()));
    });

    group.bench_function("appcommand_parse_extract_apply", |b| {
        let mut state = State::new(true);
        b.iter(|| {
            let response = Response::parse(black_box(&update), items).unwrap();
            for (definition, item) in definitions.iter().zip(response.items()) {
                definition.extract(&item, Zone::Main, &mut |field, value| {
                    black_box(state.apply_field(Zone::Main, field, value));
                });
            }
        });
    });

    let device_info = fixture("AVC-X3700H-Deviceinfo-8080.xml");
    group.bench_function("device_info_parse", |b| {
        b.iter(|| black_box(DeviceInfo::parse(black_box(&device_info)).ok()));
    });
    group.finish();
}

fn status_pages(c: &mut Criterion) {
    let status = fixture("AVR-3312-formMainZone_MainZoneXmlStatus.xml");
    let zone = fixture("AVR-3312-formZone2_Zone2XmlStatus.xml");
    let mut group = c.benchmark_group("status_page");

    group.bench_function("parse", |b| {
        b.iter(|| black_box(Page::parse(black_box(&status)).ok()));
    });

    group.bench_function("parse_extract_apply", |b| {
        let mut state = State::new(false);
        b.iter(|| {
            let status = Page::parse(black_box(&status)).ok();
            let zone_page = Page::parse(black_box(&zone)).ok();
            extract(
                status.as_ref(),
                zone_page.as_ref(),
                Zone::Zone2,
                &mut |field, value| {
                    black_box(state.apply_field(Zone::Zone2, field, value));
                },
            );
        });
    });
    group.finish();
}

fn commands(c: &mut Criterion) {
    let volume = Volume::from_db_rounded(-36.5).unwrap();
    c.bench_function("command_write_line", |b| {
        let mut line = String::with_capacity(64);
        b.iter(|| {
            for command in [
                Command::PowerOn,
                Command::SetVolume(volume),
                Command::SelectSource("GAME"),
                Command::Mute(true),
            ] {
                line.clear();
                command
                    .write_line(Brand::Denon, black_box(Zone::Main), &mut line)
                    .unwrap();
                black_box(&line);
            }
        });
    });
}

criterion_group!(benches, appcommand, status_pages, commands);
criterion_main!(benches);
