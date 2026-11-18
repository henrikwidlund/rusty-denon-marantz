//! Bytes from a HEOS connection in chunks: decoder, lines, events and payloads.
#![no_main]

use denon_marantz_heos_proto::{BrowseItem, LineDecoder, Player, QueueItem, QuickSelect, Response};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else {
        return;
    };
    // The first byte sets the size of the chunks, from 1 to 64 bytes.
    let chunk = usize::from(first % 64) + 1;
    let mut decoder = LineDecoder::new();
    for part in rest.chunks(chunk) {
        decoder.decode(part, |line| {
            let Ok(response) = Response::parse(line) else {
                return;
            };
            if let Ok(Some(event)) = response.event() {
                let _ = event.into_owned();
                return;
            }
            let _ = response.status();
            let _ = response.now_playing().map(|now| now.map(|now| now.into_owned()));
            let _ = response.payload::<Vec<Player<'_>>>();
            let _ = response.payload::<Player<'_>>();
            let _ = response.payload::<Vec<QuickSelect<'_>>>();
            let _ = response.payload::<Vec<QueueItem<'_>>>().map(|items| {
                items.into_iter().map(QueueItem::into_owned).count()
            });
            let _ = response.payload::<Vec<BrowseItem<'_>>>().map(|items| {
                items.into_iter().map(BrowseItem::into_owned).count()
            });
            let _ = response.play_mode();
            let _ = response.volume();
            let _ = response.mute();
            let _ = response.count();
        });
    }
});
