//! Speed of the telnet decoder, the message parser and the state.

use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use denon_marantz_proto::State;
use denon_marantz_proto::telnet::{DuplicateFilter, LineDecoder, Message};

/// The text lines of the parser test data, one per line, ended by `\r`.
fn stream() -> Vec<u8> {
    include_str!("../tests/data/telnet_parse.tsv")
        .lines()
        .filter_map(|row| row.split('\t').next())
        .filter(|line| !line.is_empty())
        .flat_map(|line| line.bytes().chain(Some(b'\r')))
        .collect()
}

fn decode(c: &mut Criterion) {
    let data = stream();
    let mut group = c.benchmark_group("telnet");
    group.throughput(Throughput::Bytes(data.len() as u64));

    group.bench_function("decode", |b| {
        let mut decoder = LineDecoder::new();
        b.iter(|| {
            decoder.decode(black_box(&data), |line| {
                black_box(line);
            });
        });
    });

    group.bench_function("decode_and_parse", |b| {
        let mut decoder = LineDecoder::new();
        b.iter(|| {
            decoder.decode(black_box(&data), |line| {
                black_box(Message::parse(line));
            });
        });
    });

    group.bench_function("decode_parse_filter_apply", |b| {
        let mut decoder = LineDecoder::new();
        let mut filter = DuplicateFilter::new();
        let mut state = State::new(true);
        b.iter(|| {
            decoder.decode(black_box(&data), |line| {
                if let Some(message) = Message::parse(line)
                    && filter.accept(message.parameter)
                {
                    state.apply_message(&message);
                }
            });
        });
    });

    group.bench_function("decode_in_64_byte_chunks", |b| {
        let mut decoder = LineDecoder::new();
        b.iter(|| {
            for part in black_box(&data).chunks(64) {
                decoder.decode(part, |line| {
                    black_box(line);
                });
            }
        });
    });
    group.finish();
}

/// What the receiver sends about what plays and about the signal. Most of
/// these repeat, so the value of a repeated line is the common case.
fn events(c: &mut Criterion) {
    let lines = [
        "NSE0Now Playing",
        "NSE1Song title",
        "NSE2Artist",
        "HDTITLESong",
        "SSINFSIGRES I4K50",
        "SSINFSIGCOS 22",
        "SYSDA PCM",
        "OPINFINS 22000000",
        "OPINFASP 22200000",
    ];
    let mut group = c.benchmark_group("telnet_events");
    group.throughput(Throughput::Elements(lines.len() as u64));
    group.bench_function("apply_unchanged", |b| {
        let mut state = State::new(false);
        for line in lines {
            state.apply_message(&Message::parse(line).unwrap());
        }
        b.iter(|| {
            for line in lines {
                if let Some(message) = Message::parse(black_box(line)) {
                    black_box(state.apply_message(&message));
                }
            }
        });
    });
    group.bench_function("apply_changing", |b| {
        let mut state = State::new(false);
        let mut flip = false;
        b.iter(|| {
            flip = !flip;
            let changing = if flip {
                ["NSE1Song one", "SSINFSIGRES I4K50"]
            } else {
                ["NSE1Song two", "SSINFSIGRES I1080p60"]
            };
            for line in changing {
                if let Some(message) = Message::parse(black_box(line)) {
                    black_box(state.apply_message(&message));
                }
            }
        });
    });
    group.finish();
}

criterion_group!(benches, decode, events);
criterion_main!(benches);
