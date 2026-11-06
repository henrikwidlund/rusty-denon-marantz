//! Checks `Message::parse` on lines with the expected results.
//!
//! Each row of the data file is `line`, then `-` if the line is ignored, or
//! zone, event and parameter. All fields are separated by a tab.

use denon_marantz_proto::telnet::{EventZone, Message};

const DATA: &str = include_str!("data/telnet_parse.tsv");

const fn zone_name(zone: EventZone) -> &'static str {
    match zone {
        EventZone::Main => "Main",
        EventZone::Zone2 => "Zone2",
        EventZone::Zone3 => "Zone3",
        EventZone::All => "All",
        _ => "?",
    }
}

#[test]
fn parse_gives_the_expected_events() {
    let mut rows = 0;
    for row in DATA.lines() {
        let mut fields = row.split('\t');
        let line = fields.next().unwrap();
        let expected: Vec<&str> = fields.collect();
        let actual = Message::parse(line);
        match (expected.as_slice(), actual) {
            (["-"], None) => {}
            ([zone, event, parameter], Some(message)) => {
                assert_eq!(zone_name(message.zone), *zone, "{line:?}");
                assert_eq!(message.event.code(), *event, "{line:?}");
                assert_eq!(message.parameter, *parameter, "{line:?}");
            }
            (expected, actual) => panic!("{line:?}: expected {expected:?}, got {actual:?}"),
        }
        rows += 1;
    }
    assert!(rows > 1000, "{rows}");
}
