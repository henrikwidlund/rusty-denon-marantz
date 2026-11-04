//! Compares the status page lookups with the rules of the Python library on
//! pages that real receivers sent.
//!
//! `data/status_golden.tsv` has, for each model and zone, which pages exist
//! and the values that Python found.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use denon_marantz_proto::Zone;
use denon_marantz_proto::status_page::{Page, extract, status_path, zone_path};

const NONE: &str = "<none>";

type Values = Vec<(String, Option<String>)>;

fn data(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

fn unescape(text: &str) -> Option<String> {
    if text == NONE {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(match chars.next() {
                Some('t') => '\t',
                Some('n') => '\n',
                Some('r') => '\r',
                Some(other) => other,
                None => '\\',
            });
        } else {
            out.push(c);
        }
    }
    Some(out)
}

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
fn status_pages_match_python() {
    let golden = fs::read_to_string(data("status_golden.tsv")).unwrap();
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
