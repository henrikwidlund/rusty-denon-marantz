//! Written queries and lines from the receiver into the answer tracker.
#![no_main]

use std::borrow::Cow;

use denon_marantz_proto::Brand;
use denon_marantz_proto::telnet::{AnswerLine, AnswerTracker, answer_prefix};
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
    let mut last_query = String::new();
    // A line that starts with `>` is a written query, one that starts with `!`
    // a query with a late answer, others come from the receiver.
    for line in text.split(['\r', '\n']) {
        if let Some(query) = line.strip_prefix('>') {
            tracker.written(Cow::Owned(query.to_owned()));
            query.clone_into(&mut last_query);
            written += 1;
        } else if let Some(query) = line.strip_prefix('!') {
            tracker.late_answer(Cow::Owned(query.to_owned()));
            assert!(tracker.has_answered() || answer_prefix(query).is_empty());
        } else {
            // A query without text answers nothing.
            assert!(!tracker.answers("?", line));
            if tracker.answers(&last_query, line) {
                let prefix = answer_prefix(&last_query);
                assert!(!prefix.is_empty() && line.starts_with(prefix));
            }
            // The open query with the longest matching prefix: if it answers
            // the line, `line` must take the line as an answer.
            let longest = tracker
                .open()
                .filter(|query| line.starts_with(answer_prefix(query)))
                .max_by_key(|query| answer_prefix(query).len());
            let answered = longest.is_some_and(|query| tracker.answers(query, line));
            let kind = tracker.line(line);
            if answered {
                assert_eq!(kind, AnswerLine::Answer);
            }
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
