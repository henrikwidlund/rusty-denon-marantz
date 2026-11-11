//! mDNS: the service that receivers announce and what its records tell.
//!
//! The crate does not read DNS packets. Browse for [`SERVICE_TYPE`] with an
//! mDNS library, then give the text records of each answer to
//! [`Service::from_properties`] and the addresses to [`choose_address`].

use std::net::IpAddr;

/// Service type that receivers announce.
pub const SERVICE_TYPE: &str = "_heos-audio._tcp.local.";

/// What the text records of an announcement tell about a receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Service<'a> {
    /// Model, for example `Denon AVR-X3700H`.
    pub model: Option<&'a str>,
    /// Identifier of the receiver on the network (`networkid`).
    pub network_id: Option<&'a str>,
    /// Identifier of the device (`did`).
    pub did: Option<&'a str>,
}

impl<'a> Service<'a> {
    /// Reads the text records. `property` gives the value of a key. An empty
    /// value is the same as a missing one.
    #[must_use]
    pub fn from_properties(property: impl Fn(&str) -> Option<&'a str>) -> Self {
        let get = |key| {
            property(key)
                .map(str::trim)
                .filter(|value| !value.is_empty())
        };
        Self {
            model: get("model"),
            network_id: get("networkid"),
            did: get("did"),
        }
    }
}

/// Chooses the address to reach a receiver on.
///
/// The lowest IPv4 address wins, so the choice does not depend on the order of
/// the answers. A link-local IPv4 address (169.254.x.x) comes after the others.
/// Without an IPv4 address, the lowest IPv6 address wins. An address that
/// cannot be reached from a URL is left out: unspecified, multicast, loopback
/// and IPv6 link-local addresses, which need a scope.
#[must_use]
pub fn choose_address(addresses: impl IntoIterator<Item = IpAddr>) -> Option<IpAddr> {
    addresses
        .into_iter()
        .filter(|address| match address {
            IpAddr::V4(v4) => !v4.is_unspecified() && !v4.is_multicast() && !v4.is_loopback(),
            IpAddr::V6(v6) => {
                !v6.is_unspecified()
                    && !v6.is_multicast()
                    && !v6.is_loopback()
                    && !v6.is_unicast_link_local()
            }
        })
        .min_by_key(|address| {
            let link_local = matches!(address, IpAddr::V4(v4) if v4.is_link_local());
            (address.is_ipv6(), link_local, *address)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(text: &str) -> IpAddr {
        text.parse().unwrap()
    }

    #[test]
    fn the_records_give_model_and_identifiers() {
        let records = [
            ("model", "Denon AVR-X3700H"),
            ("networkid", "abc"),
            ("did", " 123 "),
        ];
        let service = Service::from_properties(|key| {
            records.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
        });
        assert_eq!(service.model, Some("Denon AVR-X3700H"));
        assert_eq!(service.network_id, Some("abc"));
        assert_eq!(service.did, Some("123"));
    }

    #[test]
    fn missing_and_empty_records_are_none() {
        assert_eq!(Service::from_properties(|_| None), Service::default());
        assert_eq!(Service::from_properties(|_| Some("  ")), Service::default());
    }

    #[test]
    fn an_ipv4_address_wins_and_the_lowest_is_chosen() {
        let addresses = [ip("fd00::1"), ip("192.168.1.20"), ip("192.168.1.3")];
        assert_eq!(choose_address(addresses), Some(ip("192.168.1.3")));
        let reversed = [ip("192.168.1.3"), ip("192.168.1.20"), ip("fd00::1")];
        assert_eq!(choose_address(reversed), Some(ip("192.168.1.3")));
    }

    #[test]
    fn a_link_local_ipv4_address_is_the_last_choice() {
        let addresses = [ip("169.254.10.5"), ip("192.168.1.20")];
        assert_eq!(choose_address(addresses), Some(ip("192.168.1.20")));
        assert_eq!(
            choose_address([ip("169.254.10.5")]),
            Some(ip("169.254.10.5"))
        );
    }

    #[test]
    fn ipv6_is_used_without_ipv4() {
        let addresses = [ip("fd00::2"), ip("fd00::1")];
        assert_eq!(choose_address(addresses), Some(ip("fd00::1")));
    }

    #[test]
    fn addresses_that_a_url_cannot_use_are_left_out() {
        for address in [
            "0.0.0.0",
            "224.0.0.251",
            "127.0.0.1",
            "::",
            "ff02::fb",
            "::1",
            "fe80::1",
        ] {
            assert_eq!(choose_address([ip(address)]), None, "{address}");
        }
        assert_eq!(choose_address([]), None);
        assert_eq!(
            choose_address([ip("fe80::1"), ip("192.168.1.9")]),
            Some(ip("192.168.1.9"))
        );
    }
}
