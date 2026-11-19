//! Finding receivers on the network with SSDP, and with mDNS if the `mdns`
//! feature is on.

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use denon_marantz_discovery::ssdp::{self, Location, Response, SearchTarget};
use denon_marantz_proto::{Brand, DeviceDescription, ReceiverType};
use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;
use tokio::task::JoinSet;
use tokio::time::Instant;

use crate::error::Error;
use crate::http::{HttpClient, HttpConfig};
use crate::identify::{TypeProof, identify, identify_on};

#[cfg(feature = "mdns")]
mod mdns;
#[cfg(feature = "mdns")]
pub use mdns::{discover_all, discover_mdns};

/// Largest datagram that is read. SSDP answers are far smaller.
const DATAGRAM_BYTES: usize = 4096;

/// Largest device description that is read, in bytes.
const DESCRIPTION_BYTES: usize = 256 * 1024;

/// Which location addresses in the answers of a search are used.
///
/// The location is where the client fetches the device description. A device
/// on the network can put any address in it, so the client must not follow it
/// blindly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum LocationPolicy {
    /// The location is an IP address in a private range: a home or office
    /// network, loopback or link-local. This works with a router that relays
    /// SSDP between networks, because the answer then comes from the router
    /// and has the address of the receiver.
    #[default]
    LocalNetwork,
    /// The location has the address that the answer came from. The strictest
    /// choice. A relay that sends the answer from its own address breaks it.
    SameHost,
}

impl LocationPolicy {
    fn allows(self, sender: IpAddr, location: &Location) -> bool {
        let Some(address) = location.ip() else {
            return false;
        };
        match self {
            Self::LocalNetwork => is_local(address),
            Self::SameHost => address == sender,
        }
    }
}

const fn is_local(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local(),
    }
}

/// Settings of [`discover`].
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    /// Time to wait for answers after the searches are sent. Devices answer
    /// within [`ssdp::MAX_WAIT_SECONDS`].
    pub wait: Duration,
    /// Local addresses to search from. `None` uses each IPv4 address of the
    /// computer, except loopback and link-local ones.
    pub interfaces: Option<Vec<Ipv4Addr>>,
    /// Where the searches go. This is the multicast address of SSDP. It can
    /// also be the address of one receiver with port 1900.
    pub target: SocketAddrV4,
    /// Which locations in the answers are used.
    pub locations: LocationPolicy,
    /// Settings for the requests that fetch the descriptions.
    pub http: HttpConfig,
    /// Types and ports to ask for `Deviceinfo.xml` with, to see that a device is
    /// an AV receiver. `None` uses the same list as [`identify`].
    pub probes: Option<Vec<(ReceiverType, u16)>>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            wait: Duration::from_millis(u64::from(ssdp::MAX_WAIT_SECONDS) * 1000 + 500),
            interfaces: None,
            target: ssdp::MULTICAST,
            locations: LocationPolicy::default(),
            http: HttpConfig {
                request_timeout: Duration::from_secs(5),
                max_response_bytes: DESCRIPTION_BYTES,
                ..HttpConfig::default()
            },
            probes: None,
        }
    }
}

/// A receiver that answered a search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredReceiver {
    /// Address of the receiver, for [`Avr::builder`](crate::Avr::builder).
    /// An IPv6 address is without brackets. A link-local IPv6 address comes
    /// without its zone, so it does not work.
    pub host: String,
    /// Brand, from the manufacturer.
    pub brand: Brand,
    /// Manufacturer from the device description.
    pub manufacturer: String,
    /// Model from the device description.
    pub model_name: Option<String>,
    /// Name that the user gave the receiver.
    pub friendly_name: Option<String>,
    /// Serial number from the device description, or from the mDNS
    /// announcement (`did`) if it has none. `None` if it is blank.
    pub serial_number: Option<String>,
    /// MAC address of the wired network from the device information, as 12
    /// hexadecimal digits without separators.
    pub mac_address: Option<String>,
    /// Whether the answers proved the receiver type. [`TypeProof::Failed`]
    /// also if no port answered.
    pub type_proof: TypeProof,
}

/// Looks for receivers with SSDP.
///
/// It sends the searches from each local address, waits for the answers,
/// fetches the device description of each device that answered and keeps the
/// Denon and Marantz AV receivers. Other devices from the same makers, for
/// example speakers, are left out when they say that they are not an AV
/// receiver. A receiver is in the result once, also if it answered several
/// times. The result is sorted by address.
///
/// A local address that cannot send is skipped. A device whose description
/// cannot be read is skipped.
///
/// On macOS a program needs the Local Network permission to send multicast.
/// Without it every send fails with "No route to host" and this returns
/// [`Error::Network`]. Give the permission to the terminal or the app that
/// starts the program. To search without multicast, set
/// [`DiscoveryConfig::target`] to the address of the receiver and port 1900.
/// A receiver answers a search that is sent to it.
///
/// # Errors
///
/// Returns [`Error::Network`] if the local addresses cannot be read, or if no
/// search could be sent from any of them.
pub async fn discover(config: &DiscoveryConfig) -> Result<Vec<DiscoveredReceiver>, Error> {
    let interfaces = match &config.interfaces {
        Some(interfaces) => interfaces.clone(),
        None => local_addresses()?,
    };
    let mut searches = JoinSet::new();
    for local in interfaces {
        let (target, wait) = (config.target, config.wait);
        searches.spawn(async move { (local, search(local, target, wait).await) });
    }
    let mut found = Vec::new();
    let mut last_error = None;
    let mut sent = false;
    while let Some(result) = searches.join_next().await {
        let Ok((local, result)) = result else {
            continue;
        };
        match result {
            Ok(answers) => {
                sent = true;
                found.extend(answers);
            }
            Err(error) => {
                tracing::debug!(%local, %error, "SSDP search failed on this address");
                last_error = Some(error);
            }
        }
    }
    if !sent && let Some(error) = last_error {
        return Err(Error::Network {
            request: "SSDP search".to_owned(),
            source: Box::new(error),
            sent: false,
        });
    }

    let mut seen = HashSet::new();
    let mut fetches = JoinSet::new();
    for (sender, text) in found {
        let Some(location) = Location::parse(&text) else {
            tracing::debug!(location = %text, "SSDP answer with a location that is not used");
            continue;
        };
        if !config.locations.allows(sender, &location) {
            tracing::debug!(%sender, location = %text, "SSDP answer with a location that is not allowed");
            continue;
        }
        if !seen.insert((location.host.clone(), location.port, location.path.clone())) {
            continue;
        }
        let (http, probes) = (config.http, config.probes.clone());
        fetches.spawn(async move { describe(&location, http, probes.as_deref()).await });
    }
    let mut receivers = Vec::new();
    while let Some(result) = fetches.join_next().await {
        if let Ok(Described::Receiver(receiver)) = result
            && !receivers
                .iter()
                .any(|r: &DiscoveredReceiver| r.host == receiver.host)
        {
            receivers.push(receiver);
        }
    }
    sort_by_address(&mut receivers);
    Ok(receivers)
}

/// Sorts by the address as a number, so `192.168.1.3` comes before
/// `192.168.1.20`. A host that is not an address comes last.
pub(crate) fn sort_by_address(receivers: &mut [DiscoveredReceiver]) {
    receivers.sort_by_cached_key(|r| {
        let address = r.host.parse::<IpAddr>().ok();
        (address.is_none(), address, r.host.clone())
    });
}

/// The IPv4 addresses that a search is sent from.
fn local_addresses() -> Result<Vec<Ipv4Addr>, Error> {
    let interfaces = if_addrs::get_if_addrs().map_err(|source| Error::Network {
        request: "reading the local addresses".to_owned(),
        source: Box::new(source),
        sent: false,
    })?;
    let mut addresses: Vec<Ipv4Addr> = interfaces
        .iter()
        .filter_map(|interface| match interface.ip() {
            IpAddr::V4(v4) if !v4.is_loopback() && !v4.is_link_local() => Some(v4),
            _ => None,
        })
        .collect();
    addresses.sort();
    addresses.dedup();
    Ok(addresses)
}

/// Sends the searches from `local` and collects the answers: who sent it and
/// the location.
async fn search(
    local: Ipv4Addr,
    target: SocketAddrV4,
    wait: Duration,
) -> std::io::Result<Vec<(IpAddr, String)>> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    // The outgoing interface for multicast. Binding to the address is not
    // enough on every system.
    socket.set_multicast_if_v4(&local)?;
    socket.bind(&SocketAddr::V4(SocketAddrV4::new(local, 0)).into())?;
    socket.set_nonblocking(true)?;
    let socket = UdpSocket::from_std(socket.into())?;
    for search_target in SearchTarget::ALL {
        socket.send_to(search_target.request(), target).await?;
    }

    let deadline = Instant::now() + wait;
    let mut buffer = [0_u8; DATAGRAM_BYTES];
    let mut answers = Vec::new();
    while let Ok(received) = tokio::time::timeout_at(deadline, socket.recv_from(&mut buffer)).await
    {
        let (length, sender) = received?;
        if let Some(response) = Response::parse(&buffer[..length]) {
            answers.push((sender.ip(), response.location.to_owned()));
        }
    }
    Ok(answers)
}

/// Result of [`describe`].
enum Described {
    /// A Denon or Marantz AV receiver.
    Receiver(DiscoveredReceiver),
    /// The device information says that the device is not an AV receiver.
    NotReceiver,
    /// The description cannot be read or is not from a Denon or Marantz
    /// receiver.
    Unusable,
}

/// Fetches the description at the location and identifies the receiver.
async fn describe(
    location: &Location,
    http: HttpConfig,
    probes: Option<&[(ReceiverType, u16)]>,
) -> Described {
    let Ok(client) = HttpClient::new(&location.host, location.port, http) else {
        return Described::Unusable;
    };
    let description = match client
        .get_parsed(&location.path, |body| {
            DeviceDescription::parse(body).map(DeviceDescription::into_owned)
        })
        .await
    {
        Ok(description) => description,
        Err(error) => {
            tracing::debug!(%error, "description not used");
            return Described::Unusable;
        }
    };
    // A device that does not answer the device information is kept: older
    // receivers have no such page.
    let identified = match probes {
        Some(probes) => identify_on(&location.host, probes, http).await,
        None => identify(&location.host, http).await,
    };
    let (device_info, type_proof) = match identified {
        Ok(identified) => (Some(identified.device_info), identified.type_proof),
        Err(_) => (None, TypeProof::Failed),
    };
    if device_info
        .as_ref()
        .is_some_and(|device_info| !device_info.is_av_receiver())
    {
        tracing::debug!(host = %location.host, "device is not an AV receiver");
        return Described::NotReceiver;
    }
    Described::Receiver(DiscoveredReceiver {
        host: location.host.clone(),
        brand: description.brand(),
        manufacturer: description.manufacturer.to_string(),
        model_name: description.model_name.map(|v| v.to_string()),
        friendly_name: description.friendly_name.map(|v| v.to_string()),
        serial_number: description.serial_number.map(|v| v.to_string()),
        mac_address: device_info.and_then(|d| d.mac_address.map(std::borrow::Cow::into_owned)),
        type_proof,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn location(text: &str) -> Location {
        Location::parse(text).unwrap()
    }

    #[test]
    fn the_policy_decides_which_locations_are_used() {
        let sender: IpAddr = "192.168.1.1".parse().unwrap();
        let from_receiver = location("http://192.168.1.10:8080/description.xml");
        let from_sender = location("http://192.168.1.1:80/d.xml");
        let public = location("http://93.184.216.34/d.xml");
        let name = location("http://receiver.local/d.xml");
        let ipv6 = location("http://[fd00::1]/d.xml");
        let link_local = location("http://169.254.1.5/d.xml");

        // A relay answers from the router. The local network is accepted.
        let local = LocationPolicy::LocalNetwork;
        assert!(local.allows(sender, &from_receiver));
        assert!(local.allows(sender, &from_sender));
        assert!(local.allows(sender, &ipv6));
        assert!(local.allows(sender, &link_local));
        assert!(!local.allows(sender, &public));
        // A name needs a lookup, and the answer could point anywhere.
        assert!(!local.allows(sender, &name));

        let strict = LocationPolicy::SameHost;
        assert!(strict.allows(sender, &from_sender));
        assert!(!strict.allows(sender, &from_receiver));
        assert!(!strict.allows(sender, &name));
    }

    fn receiver(host: &str) -> DiscoveredReceiver {
        DiscoveredReceiver {
            host: host.to_owned(),
            brand: Brand::Denon,
            manufacturer: "Denon".to_owned(),
            model_name: None,
            friendly_name: None,
            serial_number: None,
            mac_address: None,
            type_proof: TypeProof::Read,
        }
    }

    #[test]
    fn receivers_are_sorted_by_the_address_as_a_number() {
        let mut receivers = [
            "192.168.1.20",
            "name.local",
            "192.168.1.3",
            "fd00::1",
            "10.0.0.9",
        ]
        .map(receiver)
        .to_vec();
        sort_by_address(&mut receivers);
        let hosts: Vec<_> = receivers.iter().map(|r| r.host.as_str()).collect();
        assert_eq!(
            hosts,
            [
                "10.0.0.9",
                "192.168.1.3",
                "192.168.1.20",
                "fd00::1",
                "name.local"
            ]
        );
    }

    #[test]
    fn local_addresses_have_private_ranges_only() {
        for local in [
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "127.0.0.1",
            "169.254.0.1",
            "::1",
            "fe80::1",
            "fc00::1",
        ] {
            assert!(is_local(local.parse().unwrap()), "{local}");
        }
        for public in [
            "8.8.8.8",
            "172.32.0.1",
            "192.169.0.1",
            "2001:db8::1",
            "0.0.0.0",
        ] {
            assert!(!is_local(public.parse().unwrap()), "{public}");
        }
    }
}
