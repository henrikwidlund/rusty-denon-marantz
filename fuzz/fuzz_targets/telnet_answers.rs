//! Written queries and lines from the receiver into the answer tracker.
#![no_main]

use std::borrow::Cow;

use denon_marantz_proto::Brand;
use denon_marantz_proto::telnet::AnswerTracker;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&first, rest)) = data.split_first() else {
        return;
    };
    let brand = if first & 1 == 0 {
        Brand::Denon
    } else {
        Brand::Marantz
    };
    let text = String::from_utf8_lossy(rest);
    let mut tracker = AnswerTracker::new(brand);
    let mut written = 0_usize;
    // A line that starts with `>` is a written query, others come from the receiver.
    for line in text.split(['\r', '\n']) {
        if let Some(query) = line.strip_prefix('>') {
            tracker.written(Cow::Owned(query.to_owned()));
            written += 1;
        } else {
            let _ = tracker.line(line);
        }
    }
    let still_open = tracker.open().count();
    let open = tracker.take_open().count();
    assert_eq!(open, still_open);
    assert!(open <= written);
    assert!(tracker.is_idle());
    tracker.forget_answered();
    assert!(!tracker.has_answered());
});
