//! Datagrams of the device search and locations. A location must have the
//! documented parts and must read back the same from its own URL.
#![no_main]

use std::net::IpAddr;

use denon_marantz_discovery::ssdp::{Location, Response};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(response) = Response::parse(data) {
        assert!(!response.location.is_empty());
        let _ = format!("{response:?}");
        if let Some(location) = Location::parse(response.location) {
            check(&location);
        }
    }
    if let Ok(text) = std::str::from_utf8(data)
        && let Some(location) = Location::parse(text)
    {
        check(&location);
    }
});

fn check(location: &Location) {
    let Location { host, port, path } = location;
    assert!(!host.is_empty(), "{location:?}");
    // A host name or an IPv6 address has no brackets.
    assert!(!host.contains(['[', ']']), "{location:?}");
    assert!(path.starts_with('/'), "{location:?}");
    assert_ne!(*port, 0);
    // An IPv4-mapped address is given as IPv4.
    assert!(
        !matches!(location.ip(), Some(IpAddr::V6(v6)) if v6.to_ipv4_mapped().is_some()),
        "{location:?}"
    );
    // Only an IPv6 address has a colon in the host, and the URL needs
    // brackets around it.
    let url = if host.contains(':') {
        format!("http://[{host}]:{port}{path}")
    } else {
        format!("http://{host}:{port}{path}")
    };
    assert_eq!(Location::parse(&url).as_ref(), Some(location), "{url}");
}
