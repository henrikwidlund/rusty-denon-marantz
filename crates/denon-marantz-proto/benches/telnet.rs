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

criterion_group!(benches, decode);
criterion_main!(benches);
