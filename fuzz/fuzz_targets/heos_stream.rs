//! Bytes from a HEOS connection in chunks: decoder, lines, events and payloads.
//! The lines must be the same as when the bytes come in one piece.
#![no_main]

mod chunks;

use denon_marantz_heos_proto::{BrowseItem, LineDecoder, Player, QueueItem, QuickSelect, Response};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((sizes, rest)) = chunks::split_sizes(data) else {
        return;
    };
    let mut whole = Vec::new();
    LineDecoder::new().decode(rest, |line| whole.push(line.to_owned()));

    let mut decoder = LineDecoder::new();
    let mut count = 0;
    for part in chunks::chunks(sizes, rest) {
        decoder.decode(part, |line| {
            assert_eq!(whole.get(count).map(String::as_str), Some(line));
            count += 1;
            let Ok(response) = Response::parse(line) else {
                return;
            };
            if let Ok(Some(event)) = response.event() {
                let _ = event.into_owned();
                return;
            }
            let _ = response.status();
            let _ = response
                .now_playing()
                .map(|now| now.map(|now| now.into_owned()));
            let _ = response.payload::<Vec<Player<'_>>>();
            let _ = response.payload::<Player<'_>>();
            let _ = response.payload::<Vec<QuickSelect<'_>>>();
            let _ = response
                .payload::<Vec<QueueItem<'_>>>()
                .map(|items| items.into_iter().map(QueueItem::into_owned).count());
            let _ = response
                .payload::<Vec<BrowseItem<'_>>>()
                .map(|items| items.into_iter().map(BrowseItem::into_owned).count());
            let _ = response.play_mode();
            let _ = response.volume();
            let _ = response.mute();
            let _ = response.count();
        });
    }
    assert_eq!(count, whole.len());
});
