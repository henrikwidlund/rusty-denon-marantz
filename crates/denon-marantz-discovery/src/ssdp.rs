//! SSDP: the search requests and the answers of devices.
//!
//! Send each request of [`SearchTarget::ALL`] to [`MULTICAST`] and give every
//! datagram that comes back to [`Response::parse`]. The answer has the URL of
//! the device description.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddrV4};
use std::str::FromStr;

use http::Uri;
use http::uri::PathAndQuery;

/// Address that devices listen on for searches.
pub const MULTICAST_ADDRESS: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);

/// Port that devices listen on for searches.
pub const PORT: u16 = 1900;

/// Where to send a search request.
pub const MULTICAST: SocketAddrV4 = SocketAddrV4::new(MULTICAST_ADDRESS, PORT);

/// Seconds that a device may wait before it answers. Wait this long for the
/// answers.
pub const MAX_WAIT_SECONDS: u8 = 2;

macro_rules! search_request {
    ($target:literal) => {
        concat!(
            "M-SEARCH * HTTP/1.1\r\nST: ",
            $target,
            "\r\nMX: 2\r\nMAN: \"ssdp:discover\"\r\nHOST: 239.255.255.250:1900\r\n\r\n"
        )
    };
}

/// What a search asks for. Receivers answer to different ones, so a search
/// sends all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum SearchTarget {
    /// Every device.
    All,
    /// Every root device.
    RootDevice,
    /// Every media renderer. Receivers are one.
    MediaRenderer,
}

impl SearchTarget {
    /// All the targets that a search uses.
    pub const ALL: [Self; 3] = [Self::All, Self::RootDevice, Self::MediaRenderer];

    /// The value of the `ST` header.
    #[must_use]
    pub const fn value(self) -> &'static str {
        match self {
            Self::All => "ssdp:all",
            Self::RootDevice => "upnp:rootdevice",
            Self::MediaRenderer => "urn:schemas-upnp-org:device:MediaRenderer:1",
        }
    }

    /// The datagram to send to [`MULTICAST`].
    #[must_use]
    pub const fn request(self) -> &'static [u8] {
        match self {
            Self::All => search_request!("ssdp:all").as_bytes(),
            Self::RootDevice => search_request!("upnp:rootdevice").as_bytes(),
            Self::MediaRenderer => {
                search_request!("urn:schemas-upnp-org:device:MediaRenderer:1").as_bytes()
            }
        }
    }
}

/// A device that answered a search, or told that it is on the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Response<'a> {
    /// URL of the device description, as the device wrote it. Read it with
    /// [`Location::parse`].
    pub location: &'a str,
    /// Kind of device that the answer is about (`ST` or `NT`).
    pub target: Option<&'a str>,
    /// Unique name of the device and the target (`USN`).
    pub usn: Option<&'a str>,
    /// Software of the device (`SERVER`).
    pub server: Option<&'a str>,
}

impl<'a> Response<'a> {
    /// Reads a datagram. Returns `None` if it is not an answer to a search or
    /// a message that a device is on the network, if it has no location, or
    /// if it is not text.
    ///
    /// Searches of other programs and messages that a device is leaving are
    /// not answers, so they give `None`.
    #[must_use]
    pub fn parse(datagram: &'a [u8]) -> Option<Self> {
        let text = std::str::from_utf8(datagram).ok()?;
        let mut lines = text.lines();
        let start = lines.next()?.trim();
        let is_notification = start.eq_ignore_ascii_case("NOTIFY * HTTP/1.1");
        let is_answer = start
            .get(..12)
            .is_some_and(|head| head.eq_ignore_ascii_case("HTTP/1.1 200"));
        if !is_notification && !is_answer {
            return None;
        }

        let (mut location, mut target, mut usn, mut server) = (None, None, None, None);
        let mut alive = false;
        for line in lines.take_while(|line| !line.trim().is_empty()) {
            let Some((name, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            let slot = match name.trim().to_ascii_uppercase().as_str() {
                "LOCATION" => &mut location,
                "ST" | "NT" => &mut target,
                "USN" => &mut usn,
                "SERVER" => &mut server,
                "NTS" => {
                    alive = value.eq_ignore_ascii_case("ssdp:alive");
                    continue;
                }
                _ => continue,
            };
            // The first header of a name counts.
            slot.get_or_insert(value);
        }
        if is_notification && !alive {
            return None;
        }
        Some(Self {
            location: location.filter(|value| !value.is_empty())?,
            target,
            usn,
            server,
        })
    }
}

/// The parts of a location that a client needs to fetch the description.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// Host name or IP address. An IPv6 address has no brackets. An
    /// IPv4-mapped IPv6 address is given as its IPv4 address.
    pub host: String,
    /// Port, 80 if the URL has none.
    pub port: u16,
    /// Path and query, at least `/`.
    pub path: String,
}

impl Location {
    /// Reads an `http` URL. Returns `None` for any other kind of URL, for a
    /// URL with a user name, port 0, no host or brackets that do not hold an
    /// IPv6 address, and for text that is not a URL.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let uri = Uri::from_str(text).ok()?;
        if uri.scheme_str()? != "http" {
            return None;
        }
        let authority = uri.authority()?;
        if authority.as_str().contains('@') {
            return None;
        }
        let host = authority.host();
        let host = match host.strip_prefix('[') {
            Some(rest) => {
                let inner = rest.strip_suffix(']')?;
                let address = inner.parse::<Ipv6Addr>().ok()?;
                address
                    .to_ipv4_mapped()
                    .map_or_else(|| inner.to_owned(), |v4| v4.to_string())
            }
            None if host.is_empty() || host.contains(['[', ']']) => return None,
            None => host.to_owned(),
        };
        // The `http` crate drops a port that is too large, so read it here.
        let port = match authority.as_str().rsplit_once(':') {
            Some((_, digits))
                if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()) =>
            {
                digits.parse().ok().filter(|&port| port != 0)?
            }
            _ => 80,
        };
        let path = match uri.path_and_query().map(PathAndQuery::as_str) {
            Some(path) if path.starts_with('/') => path.to_owned(),
            // Only a query, for example `http://host?x`.
            Some(path) => format!("/{path}"),
            None => "/".to_owned(),
        };
        Some(Self { host, port, path })
    }

    /// The host as an IP address, if it is one.
    #[must_use]
    pub fn ip(&self) -> Option<IpAddr> {
        self.host.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = "HTTP/1.1 200 OK\r\n\
        CACHE-CONTROL: max-age=1800\r\n\
        EXT:\r\n\
        LOCATION: http://192.168.1.10:8080/description.xml\r\n\
        SERVER: Linux UPnP/1.0 Denon-Heos/4.0.0\r\n\
        ST: urn:schemas-upnp-org:device:MediaRenderer:1\r\n\
        USN: uuid:1234::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";

    #[test]
    fn requests_have_the_target_and_the_wait() {
        for target in SearchTarget::ALL {
            let request = std::str::from_utf8(target.request()).unwrap();
            assert!(request.starts_with("M-SEARCH * HTTP/1.1\r\n"), "{request}");
            assert!(request.contains(&format!("\r\nST: {}\r\n", target.value())));
            assert!(request.contains(&format!("\r\nMX: {MAX_WAIT_SECONDS}\r\n")));
            assert!(request.contains("\r\nMAN: \"ssdp:discover\"\r\n"));
            assert!(request.contains(&format!("\r\nHOST: {MULTICAST}\r\n")));
            assert!(request.ends_with("\r\n\r\n"));
        }
    }

    #[test]
    fn an_answer_has_its_values() {
        let response = Response::parse(ANSWER.as_bytes()).unwrap();
        assert_eq!(
            response.location,
            "http://192.168.1.10:8080/description.xml"
        );
        assert_eq!(
            response.target,
            Some("urn:schemas-upnp-org:device:MediaRenderer:1")
        );
        assert_eq!(
            response.usn,
            Some("uuid:1234::urn:schemas-upnp-org:device:MediaRenderer:1")
        );
        assert_eq!(response.server, Some("Linux UPnP/1.0 Denon-Heos/4.0.0"));
    }

    #[test]
    fn header_names_are_not_case_sensitive_and_line_ends_can_be_short() {
        let text = "HTTP/1.1 200 OK\nlocation:   http://10.0.0.5/d.xml  \nSt: upnp:rootdevice\n\n";
        let response = Response::parse(text.as_bytes()).unwrap();
        assert_eq!(response.location, "http://10.0.0.5/d.xml");
        assert_eq!(response.target, Some("upnp:rootdevice"));
        assert_eq!(response.usn, None);
    }

    #[test]
    fn the_first_header_of_a_name_counts() {
        let text = "HTTP/1.1 200 OK\r\nLOCATION: http://a/x\r\nLOCATION: http://b/y\r\n\r\n";
        assert_eq!(
            Response::parse(text.as_bytes()).unwrap().location,
            "http://a/x"
        );
    }

    #[test]
    fn a_device_that_is_on_the_network_is_an_answer() {
        let alive = "NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\n\
            NT: upnp:rootdevice\r\nNTS: ssdp:alive\r\nLOCATION: http://10.0.0.5:80/d.xml\r\n\r\n";
        let response = Response::parse(alive.as_bytes()).unwrap();
        assert_eq!(response.location, "http://10.0.0.5:80/d.xml");
        assert_eq!(response.target, Some("upnp:rootdevice"));
    }

    #[test]
    fn other_datagrams_are_not_answers() {
        let leaving = "NOTIFY * HTTP/1.1\r\nNT: upnp:rootdevice\r\nNTS: ssdp:byebye\r\n\
            LOCATION: http://10.0.0.5/d.xml\r\n\r\n";
        let no_nts = "NOTIFY * HTTP/1.1\r\nLOCATION: http://10.0.0.5/d.xml\r\n\r\n";
        let search = std::str::from_utf8(SearchTarget::All.request()).unwrap();
        let not_found = "HTTP/1.1 404 Not Found\r\nLOCATION: http://10.0.0.5/d.xml\r\n\r\n";
        let no_location = "HTTP/1.1 200 OK\r\nST: upnp:rootdevice\r\n\r\n";
        let empty_location = "HTTP/1.1 200 OK\r\nLOCATION:\r\n\r\n";
        // A header after the blank line is not part of the message.
        let late = "HTTP/1.1 200 OK\r\n\r\nLOCATION: http://10.0.0.5/d.xml\r\n";
        for text in [
            leaving,
            no_nts,
            search,
            not_found,
            no_location,
            empty_location,
            late,
            "",
        ] {
            assert_eq!(Response::parse(text.as_bytes()), None, "{text:?}");
        }
        assert_eq!(Response::parse(&[0xFF, 0xFE, b'\r', b'\n']), None);
    }

    #[test]
    fn locations() {
        let location = |text| Location::parse(text);
        let expected = |host: &str, port, path: &str| {
            Some(Location {
                host: host.to_owned(),
                port,
                path: path.to_owned(),
            })
        };
        assert_eq!(
            location("http://192.168.1.10:8080/description.xml"),
            expected("192.168.1.10", 8080, "/description.xml")
        );
        assert_eq!(
            location("http://192.168.1.10/"),
            expected("192.168.1.10", 80, "/")
        );
        assert_eq!(
            location("http://192.168.1.10"),
            expected("192.168.1.10", 80, "/")
        );
        assert_eq!(
            location("HTTP://receiver.local:80/a/b?c=1"),
            expected("receiver.local", 80, "/a/b?c=1")
        );
        assert_eq!(
            location("http://[fe80::1]:8080/d"),
            expected("fe80::1", 8080, "/d")
        );
        assert_eq!(
            location("http://192.168.1.10?x=1"),
            expected("192.168.1.10", 80, "/?x=1")
        );
        assert_eq!(
            location("http://[::ffff:192.168.1.10]:8080/d"),
            expected("192.168.1.10", 8080, "/d")
        );
        for bad in [
            "",
            "192.168.1.10/d.xml",
            "https://192.168.1.10/d.xml",
            "ftp://192.168.1.10/d.xml",
            "http://user:pass@192.168.1.10/d.xml",
            "http://user@192.168.1.10/d.xml",
            "http:///d.xml",
            "/description.xml",
            "http://192.168.1.10:99999/d.xml",
            "http://192.168.1.10:0/d.xml",
            "http://://tt/",
            "http://z[:]",
            "http://[%(]",
            "http://[fe80::1%25en0]/d",
            "http://a b/",
        ] {
            assert_eq!(location(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_host_can_be_an_ip_address() {
        let ip = |text| Location::parse(text).unwrap().ip();
        assert_eq!(
            ip("http://192.168.1.10:8080/d"),
            Some("192.168.1.10".parse().unwrap())
        );
        assert_eq!(ip("http://[fe80::1]/d"), Some("fe80::1".parse().unwrap()));
        assert_eq!(ip("http://receiver.local/d"), None);
    }

    mod no_panic {
        use proptest::prelude::*;

        use super::super::{Location, Response};

        proptest! {
            #[test]
            fn any_bytes_are_safe(data in proptest::collection::vec(any::<u8>(), 0..2048)) {
                let _ = Response::parse(&data);
            }

            #[test]
            fn any_text_is_a_safe_location(text in "\\PC{0,200}") {
                let _ = Location::parse(&text);
            }

            #[test]
            fn answers_with_any_header_values_are_safe(value in "[ -~]{0,100}") {
                let text = format!("HTTP/1.1 200 OK\r\nLOCATION: {value}\r\nST: {value}\r\n\r\n");
                if let Some(response) = Response::parse(text.as_bytes()) {
                    let _ = Location::parse(response.location);
                }
            }
        }
    }
}
