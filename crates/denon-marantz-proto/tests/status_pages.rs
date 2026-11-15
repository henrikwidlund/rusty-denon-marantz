//! Checks the status page lookups on pages that real receivers sent.
//!
//! `data/status_golden.tsv` has, for each model and zone, which pages exist
//! and the expected values.

mod common;

use common::{data, read_data, unescape};

use std::collections::BTreeMap;
use std::fs;

use denon_marantz_proto::Zone;
use denon_marantz_proto::status_page::{Page, extract, sources, status_path, zone_path};

type Values = Vec<(String, Option<String>)>;

fn zone(name: &str) -> Zone {
    Zone::from_name(name).unwrap()
}

/// File of a page. The fixtures are named `<model>-<page name>.xml`.
fn file(model: &str, path: &str) -> String {
    let page = path
        .trim_start_matches("/goform/")
        .split('?')
        .next()
        .unwrap()
        .trim_end_matches(".xml");
    format!("xml/{model}-{page}.xml")
}

/// Pages that are missing or that are error pages count as not read.
fn parse<'a>(body: Option<&'a [u8]>, html_pages: &mut usize) -> Option<Page<'a>> {
    let page = body.map(Page::parse);
    if matches!(page, Some(Err(_))) {
        *html_pages += 1;
    }
    page.and_then(Result::ok)
}

#[test]
fn status_pages_have_the_expected_values() {
    let golden = read_data("status_golden.tsv");
    let mut cases = Vec::new();
    let mut expected: BTreeMap<(String, String), Values> = BTreeMap::new();
    for row in golden.lines() {
        let f: Vec<&str> = row.split('\t').collect();
        match f[0] {
            "P" => cases.push((f[1].to_owned(), f[2].to_owned(), f[3].to_owned())),
            "R" => expected
                .entry((f[1].to_owned(), f[2].to_owned()))
                .or_default()
                .push((f[3].to_owned(), unescape(f[4]))),
            other => panic!("unknown row {other}"),
        }
    }

    let (mut compared, mut html_pages) = (0, 0);
    for (model, zone_name, pages) in &cases {
        let zone = zone(zone_name);
        let read = |file: String| fs::read(data(&file)).ok();
        let status_body = read(file(model, status_path(zone)));
        let zone_body = read(file(model, zone_path(zone)));
        let (status, zone_page) = (
            parse(status_body.as_deref(), &mut html_pages),
            parse(zone_body.as_deref(), &mut html_pages),
        );
        assert_eq!(
            (status.is_some(), zone_page.is_some()),
            (pages.starts_with('S'), pages.ends_with('S')),
            "{model} {zone_name}: pages"
        );

        let mut got: Values = Vec::new();
        extract(
            status.as_ref(),
            zone_page.as_ref(),
            zone,
            &mut |field, value| {
                got.push((format!("{field:?}"), value.map(str::to_owned)));
            },
        );
        got.sort();
        let mut want = expected
            .remove(&(model.clone(), zone_name.clone()))
            .unwrap_or_default();
        want.sort();
        assert_eq!(got, want, "{model} {zone_name}");
        compared += want.len();
    }
    assert!(expected.is_empty());
    assert!(
        cases.len() >= 60 && compared > 150 && html_pages > 0,
        "{} {compared} {html_pages}",
        cases.len()
    );
}

type Source = (String, Option<String>, Option<bool>);

/// The sources of the main zone pages of a model.
fn sources_of(model: &str) -> Vec<Source> {
    let read = |path| fs::read(data(&file(model, path))).ok();
    let (status, zone_page) = (read(status_path(Zone::Main)), read(zone_path(Zone::Main)));
    let mut html_pages = 0;
    let status = parse(status.as_deref(), &mut html_pages);
    let zone_page = parse(zone_page.as_deref(), &mut html_pages);
    let mut found = Vec::new();
    sources(
        status.as_ref(),
        zone_page.as_ref(),
        |name, rename, in_use| {
            found.push((name.to_owned(), rename.map(str::to_owned), in_use));
        },
    );
    found
}

fn source(name: &str, rename: Option<&str>, in_use: Option<bool>) -> Source {
    (name.to_owned(), rename.map(str::to_owned), in_use)
}

#[test]
fn source_lists_are_read() {
    // Names inside a second `value` element, on the status page.
    let x1100w = sources_of("AVR-X1100W");
    assert_eq!(x1100w.len(), 11);
    assert_eq!(x1100w[0], source("CBL/SAT", Some("Kodi TV"), Some(true)));
    assert_eq!(x1100w[3], source("GAME", Some("Game"), Some(false)));
    // The status page has no `SourceDelete`, so the zone page is used.
    // `SOURCE` is not an input.
    let avr3311 = sources_of("AVR-3311CI");
    assert_eq!(avr3311.len(), 14);
    assert_eq!(avr3311[0], source("TUNER", None, Some(false)));
    assert_eq!(avr3311[2], source("CD", Some("SqzBox"), Some(true)));
    // A model without the lists.
    assert_eq!(sources_of("DRA-N4"), []);
}
