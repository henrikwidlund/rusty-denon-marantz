//! Finding receivers with mDNS.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use denon_marantz_discovery::mdns::{SERVICE_TYPE, Service, choose_address};
use denon_marantz_proto::{Brand, ReceiverType};
use mdns_sd::{ResolvedService, ScopedIp, ServiceDaemon, ServiceEvent};
use tokio::time::Instant;

use super::{DiscoveredReceiver, DiscoveryConfig, Location, describe, discover, is_local};
use crate::error::Error;
use crate::identify::{identify, identify_on};

/// Most services that are used. An announcement can name any number of them.
const MAX_SERVICES: usize = 64;

/// Types that differ in where the UPnP description is.
const DESCRIPTION_TYPES: [ReceiverType; 2] = [ReceiverType::Avr, ReceiverType::AvrX2016];

/// A service that was announced, with the values that are used.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Announced {
    address: IpAddr,
    model: Option<String>,
    did: Option<String>,
}

impl Announced {
    fn from_resolved(service: &ResolvedService) -> Option<Self> {
        Self::from_parts(
            service.get_addresses().iter().map(ScopedIp::to_ip_addr),
            |key| service.get_property_val_str(key),
        )
    }

    fn from_parts<'a>(
        addresses: impl IntoIterator<Item = IpAddr>,
        property: impl Fn(&str) -> Option<&'a str>,
    ) -> Option<Self> {
        let records = Service::from_properties(property);
        Some(Self {
            // Only local addresses, as with SSDP: an announcement can carry any
            // address, and the client must not request from a public one.
            address: choose_address(addresses.into_iter().filter(|a| is_local(*a)))?,
            model: records.model.map(str::to_owned),
            did: records.did.map(str::to_owned),
        })
    }
}

/// Stops the browsing thread when it goes out of scope, also if the future is
/// dropped.
struct Daemon(ServiceDaemon);

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.0.shutdown();
    }
}

fn network_error(request: &str, source: mdns_sd::Error) -> Error {
    Error::Network {
        request: request.to_owned(),
        source: Box::new(source),
    }
}

/// Looks for receivers with mDNS.
///
/// It browses for the service that receivers announce, waits for
/// [`DiscoveryConfig::wait`], and reads the device description of each service
/// that answered. A device that is not an AV receiver is left out. A receiver
/// whose description cannot be read is kept if it answers the device
/// information, and then has only the model and no friendly name. The serial
/// number is the one in the description, or the device identifier of the
/// announcement if the description has none. The result is sorted by address.
///
/// Only [`DiscoveryConfig::wait`], [`DiscoveryConfig::http`] and
/// [`DiscoveryConfig::probes`] are used. [`DiscoveryConfig::interfaces`],
/// [`DiscoveryConfig::target`] and [`DiscoveryConfig::locations`] do not apply.
/// mDNS goes out on every interface of the computer. Only local addresses are
/// used, and at most 64 services.
///
/// The sockets open on a thread after the call starts. If they cannot open, for
/// example without the Local Network permission on macOS, nothing is
/// announced and the result is empty, not an error.
///
/// # Errors
///
/// Returns [`Error::Network`] if the mDNS service cannot be created.
pub async fn discover_mdns(config: &DiscoveryConfig) -> Result<Vec<DiscoveredReceiver>, Error> {
    let mut announced = browse(config).await?;
    // Several names for one address need one request only.
    let mut addresses = HashSet::new();
    announced.retain(|a| addresses.insert(a.address));
    let mut fetches = tokio::task::JoinSet::new();
    for announcement in announced {
        let (http, probes) = (config.http, config.probes.clone());
        fetches.spawn(async move { receiver(&announcement, http, probes.as_deref()).await });
    }
    let mut receivers: Vec<DiscoveredReceiver> = Vec::new();
    while let Some(result) = fetches.join_next().await {
        if let Ok(Some(found)) = result
            && !receivers.iter().any(|r| r.host == found.host)
        {
            receivers.push(found);
        }
    }
    receivers.sort_by(|a, b| a.host.cmp(&b.host));
    Ok(receivers)
}

/// Looks for receivers with SSDP and mDNS and joins the answers.
///
/// A receiver that both find is in the result once, with the values of SSDP.
/// It counts as the same receiver if the address or the serial number is the
/// same. The result is sorted by address.
///
/// # Errors
///
/// Returns the error of [`discover`] if it failed and the result is empty, so
/// that a missing permission is not hidden by an empty mDNS answer.
pub async fn discover_all(config: &DiscoveryConfig) -> Result<Vec<DiscoveredReceiver>, Error> {
    let (ssdp, mdns) = tokio::join!(discover(config), discover_mdns(config));
    combine(ssdp, mdns)
}

/// Joins the results of the two searches.
fn combine(
    ssdp: Result<Vec<DiscoveredReceiver>, Error>,
    mdns: Result<Vec<DiscoveredReceiver>, Error>,
) -> Result<Vec<DiscoveredReceiver>, Error> {
    if let Err(error) = &mdns {
        tracing::debug!(%error, "mDNS discovery failed");
    }
    match (ssdp, mdns) {
        (Ok(first), Ok(second)) => Ok(join(first, second)),
        (Ok(first), Err(_)) => Ok(first),
        (Err(error), Ok(second)) if second.is_empty() => Err(error),
        (Err(error), Ok(second)) => {
            tracing::debug!(%error, "SSDP discovery failed");
            Ok(second)
        }
        (Err(error), Err(_)) => Err(error),
    }
}

/// Joins two lists. A receiver in both has the values of the first. It is the
/// same receiver if the address or a serial number is the same.
fn join(
    first: Vec<DiscoveredReceiver>,
    second: Vec<DiscoveredReceiver>,
) -> Vec<DiscoveredReceiver> {
    let mut all = first;
    for receiver in second {
        let same = |r: &DiscoveredReceiver| {
            r.host == receiver.host
                || (r.serial_number.is_some() && r.serial_number == receiver.serial_number)
        };
        if !all.iter().any(same) {
            all.push(receiver);
        }
    }
    all.sort_by(|a, b| a.host.cmp(&b.host));
    all
}

/// Browses until the wait is over.
async fn browse(config: &DiscoveryConfig) -> Result<Vec<Announced>, Error> {
    let daemon = Daemon(ServiceDaemon::new().map_err(|e| network_error("mDNS start", e))?);
    let events = daemon
        .0
        .browse(SERVICE_TYPE)
        .map_err(|e| network_error("mDNS browse", e))?;
    let deadline = Instant::now() + config.wait;
    let mut found: HashMap<String, Announced> = HashMap::new();
    while let Ok(Ok(event)) = tokio::time::timeout_at(deadline, events.recv_async()).await {
        match &event {
            ServiceEvent::SearchStarted(kind) => tracing::debug!(kind, "mDNS search started"),
            ServiceEvent::ServiceFound(_, name) => tracing::debug!(name, "mDNS service found"),
            _ => {}
        }
        if let ServiceEvent::ServiceRemoved(_, name) = &event {
            found.remove(name);
        }
        if let ServiceEvent::ServiceResolved(service) = event {
            match Announced::from_resolved(&service) {
                Some(_)
                    if found.len() >= MAX_SERVICES && !found.contains_key(&service.fullname) =>
                {
                    tracing::debug!(name = %service.fullname, "mDNS service ignored, too many");
                }
                Some(announced) => {
                    tracing::debug!(name = %service.fullname, address = %announced.address, "mDNS service resolved");
                    found.insert(service.fullname.clone(), announced);
                }
                None => {
                    tracing::debug!(name = %service.fullname, "mDNS service without a usable address");
                }
            }
        }
    }
    tracing::debug!(count = found.len(), "mDNS search ended");
    Ok(found.into_values().collect())
}

/// Reads the description of an announced receiver.
async fn receiver(
    announced: &Announced,
    http: crate::http::HttpConfig,
    probes: Option<&[(ReceiverType, u16)]>,
) -> Option<DiscoveredReceiver> {
    let host = announced.address.to_string();
    let location = |receiver_type: ReceiverType| Location {
        host: host.clone(),
        port: receiver_type.description_port(),
        path: receiver_type.description_path().to_owned(),
    };
    let locations = DESCRIPTION_TYPES.map(location);
    let mut described = None;
    for location in &locations {
        described = describe(location, http, probes).await;
        if described.is_some() {
            break;
        }
    }
    if let Some(mut found) = described {
        found.serial_number = found.serial_number.or_else(|| announced.did.clone());
        return Some(found);
    }
    // No description. A receiver that answers the device information is kept.
    let identified = match probes {
        Some(probes) => identify_on(&host, probes, http).await,
        None => identify(&host, http).await,
    }
    .ok()?;
    if !identified.device_info.is_av_receiver() {
        return None;
    }
    // The model text has the brand only on some receivers. Marantz needs its name.
    let brand = if announced
        .model
        .as_deref()
        .is_some_and(|model| model.to_ascii_lowercase().contains("marantz"))
    {
        Brand::Marantz
    } else {
        Brand::Denon
    };
    Some(DiscoveredReceiver {
        host,
        brand,
        manufacturer: brand.name().to_owned(),
        model_name: identified
            .device_info
            .model_name
            .map(std::borrow::Cow::into_owned)
            .or_else(|| announced.model.clone()),
        friendly_name: None,
        serial_number: announced.did.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn receiver(host: &str, name: &str) -> DiscoveredReceiver {
        DiscoveredReceiver {
            host: host.to_owned(),
            brand: Brand::Denon,
            manufacturer: "Denon".to_owned(),
            model_name: None,
            friendly_name: Some(name.to_owned()),
            serial_number: None,
        }
    }

    #[test]
    fn a_receiver_in_both_lists_has_the_values_of_the_first() {
        let first = vec![receiver("192.168.1.20", "ssdp")];
        let second = vec![
            receiver("192.168.1.20", "mdns"),
            receiver("192.168.1.3", "mdns"),
        ];
        let all = join(first, second);
        let names: Vec<_> = all
            .iter()
            .map(|r| (r.host.as_str(), r.friendly_name.as_deref().unwrap()))
            .collect();
        assert_eq!(names, [("192.168.1.20", "ssdp"), ("192.168.1.3", "mdns")]);
    }

    #[test]
    fn a_receiver_with_the_same_serial_is_the_same() {
        let mut ssdp = receiver("192.168.1.20", "ssdp");
        ssdp.serial_number = Some("S1".to_owned());
        let mut mdns = receiver("192.168.1.3", "mdns");
        mdns.serial_number = Some("S1".to_owned());
        let other = receiver("192.168.1.4", "other");
        assert_eq!(join(vec![ssdp], vec![mdns, other]).len(), 2);
    }

    fn network_failure() -> Error {
        Error::Network {
            request: "SSDP search".to_owned(),
            source: Box::new(std::io::Error::other("no route")),
        }
    }

    #[test]
    fn a_failed_ssdp_search_is_reported_if_nothing_else_was_found() {
        let found = vec![receiver("192.168.1.3", "mdns")];
        assert!(combine(Err(network_failure()), Ok(Vec::new())).is_err());
        assert!(combine(Err(network_failure()), Err(network_failure())).is_err());
        assert_eq!(
            combine(Err(network_failure()), Ok(found.clone())).unwrap(),
            found
        );
        assert_eq!(
            combine(Ok(found.clone()), Err(network_failure())).unwrap(),
            found
        );
        assert_eq!(combine(Ok(Vec::new()), Ok(Vec::new())).unwrap(), []);
    }

    #[test]
    fn the_records_and_the_address_of_a_service_are_read() {
        let records = [
            ("model", "Denon AVR-X3700H"),
            ("networkid", "n1"),
            ("did", "d1"),
        ];
        let property = |key: &str| records.iter().find(|(k, _)| *k == key).map(|(_, v)| *v);
        let addresses = ["fd00::1", "192.168.1.20"].map(|a| a.parse::<IpAddr>().unwrap());
        let announced = Announced::from_parts(addresses, property).unwrap();
        assert_eq!(announced.address, "192.168.1.20".parse::<IpAddr>().unwrap());
        assert_eq!(announced.model.as_deref(), Some("Denon AVR-X3700H"));
        assert_eq!(announced.did.as_deref(), Some("d1"));
    }

    #[test]
    fn a_public_address_is_never_used() {
        let public = ["93.184.216.34", "2001:db8::1"].map(|a| a.parse::<IpAddr>().unwrap());
        assert_eq!(Announced::from_parts(public, |_| None), None);
        let mixed = ["93.184.216.34", "192.168.1.9"].map(|a| a.parse::<IpAddr>().unwrap());
        let announced = Announced::from_parts(mixed, |_| None).unwrap();
        assert_eq!(announced.address, "192.168.1.9".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn a_service_without_a_usable_address_is_left_out() {
        let addresses = ["fe80::1".parse::<IpAddr>().unwrap()];
        assert_eq!(Announced::from_parts(addresses, |_| None), None);
    }
}
