//! The address choice of mDNS answers. The result must follow the documented
//! rules and must not depend on the order of the addresses.
#![no_main]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use denon_marantz_discovery::mdns::choose_address;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Each address is a kind byte and then 4 or 16 bytes: IPv4, IPv4 mapped
    // to IPv6, or IPv6.
    let mut addresses = Vec::new();
    let mut rest = data;
    while let Some((&kind, tail)) = rest.split_first()
        && addresses.len() < 64
    {
        let (address, tail) = match kind % 3 {
            0 | 1 => {
                let Some((bytes, tail)) = tail.split_first_chunk::<4>() else {
                    break;
                };
                let v4 = Ipv4Addr::from(*bytes);
                let address = if kind % 3 == 0 {
                    IpAddr::V4(v4)
                } else {
                    IpAddr::V6(v4.to_ipv6_mapped())
                };
                (address, tail)
            }
            _ => {
                let Some((bytes, tail)) = tail.split_first_chunk::<16>() else {
                    break;
                };
                (IpAddr::V6(Ipv6Addr::from(*bytes)), tail)
            }
        };
        addresses.push(address);
        rest = tail;
    }

    let chosen = choose_address(addresses.iter().copied());
    assert_eq!(chosen, choose_address(addresses.iter().rev().copied()));
    let expected = addresses
        .iter()
        .map(|a| a.to_canonical())
        .filter(|&address| reachable(address))
        .min_by_key(|&address| {
            let link_local = matches!(address, IpAddr::V4(v4) if v4.is_link_local());
            (address.is_ipv6(), link_local, address)
        });
    assert_eq!(chosen, expected, "{addresses:?}");
    if let Some(address) = chosen {
        assert!(addresses.iter().any(|a| a.to_canonical() == address));
        assert!(reachable(address));
        if let IpAddr::V6(v6) = address {
            assert!(v6.to_ipv4_mapped().is_none());
        }
    }
});

/// The documented filter: an address that a URL can reach.
fn reachable(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => {
            !v4.is_unspecified() && !v4.is_multicast() && !v4.is_broadcast() && !v4.is_loopback()
        }
        IpAddr::V6(v6) => {
            !v6.is_unspecified()
                && !v6.is_multicast()
                && !v6.is_loopback()
                && !v6.is_unicast_link_local()
        }
    }
}
