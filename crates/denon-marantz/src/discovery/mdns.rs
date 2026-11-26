//! Finding receivers with mDNS.

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;

use denon_marantz_discovery::mdns::{SERVICE_TYPE, Service, choose_address};
use denon_marantz_proto::{Brand, ReceiverType};
use mdns_sd::{ResolvedService, ScopedIp, ServiceDaemon, ServiceEvent};
use tokio::time::Instant;

use super::{
    Described, DiscoveredReceiver, DiscoveryConfig, Location, describe, is_local, search_ssdp,
    sort_by_address,
};
use crate::error::Error;
use crate::identify::{Identified, identify, identify_on};

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
        sent: false,
    }
}

/// Looks for receivers with mDNS.
///
/// It browses for [`DiscoveryConfig::wait`] for the service that receivers
/// announce, and reads the device description of each service as soon as it
/// is announced. The values of the first announcement for an address are
/// used. A service that is removed before the wait ends is left out. A device
/// that is not an AV receiver is left out. A receiver whose description
/// cannot be read is kept if the device information proves that it is an AV
/// receiver, and then has only the model and no friendly name. The serial
/// number is the one in the description, or the device identifier of the
/// announcement if the description has none. The result is sorted by
/// address.
///
/// Only [`DiscoveryConfig::wait`], [`DiscoveryConfig::http`] and
/// [`DiscoveryConfig::probes`] are used. [`DiscoveryConfig::interfaces`],
/// [`DiscoveryConfig::target`] and [`DiscoveryConfig::locations`] do not apply.
/// mDNS goes out on every interface of the computer. Only local addresses are
/// used, and at most 64 services and 64 addresses in one call.
///
/// The sockets open on a thread after the call starts. If they cannot open, for
/// example without the Local Network permission on macOS, nothing is
/// announced and the result is empty, not an error.
///
/// # Errors
///
/// Returns [`Error::Network`] if the mDNS service cannot be created.
pub async fn discover_mdns(config: &DiscoveryConfig) -> Result<Vec<DiscoveredReceiver>, Error> {
    let mut receivers = Vec::new();
    search_mdns(config, &|_| false, &mut receivers).await?;
    // Two addresses of one receiver have the same serial number.
    Ok(join(Vec::new(), receivers))
}

/// Looks for receivers with SSDP and mDNS and joins the answers.
///
/// A receiver that both find is in the result once, with the values of SSDP.
/// It counts as the same receiver if the address or the serial number is the
/// same. The result is sorted by address.
///
/// # Errors
///
/// Returns the error of [`discover`](super::discover) if it failed and the
/// result is empty, so that a missing permission is not hidden by an empty
/// mDNS answer.
pub async fn discover_all(config: &DiscoveryConfig) -> Result<Vec<DiscoveredReceiver>, Error> {
    discover_all_until(config, |_| false).await
}

/// As [`discover_all`], but ends as soon as `until` gives true for a receiver
/// that one of the two searches found.
///
/// The result then has the receivers that were found until then, the one that
/// matched included, and is not an error. If no receiver matches, the result
/// is the same as that of [`discover_all`]: the searches wait for answers for
/// [`DiscoveryConfig::wait`], and then for the descriptions that are still
/// being read.
///
/// For example, to find a receiver by its serial number:
///
/// ```no_run
/// # async fn example() -> Result<(), denon_marantz::Error> {
/// use denon_marantz::{DiscoveryConfig, discover_all_until};
///
/// let found = discover_all_until(&DiscoveryConfig::default(), |r| {
///     r.serial_number.as_deref() == Some("ABC0123456789")
/// })
/// .await?;
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// As [`discover_all`].
pub async fn discover_all_until(
    config: &DiscoveryConfig,
    until: impl Fn(&DiscoveredReceiver) -> bool + Sync,
) -> Result<Vec<DiscoveredReceiver>, Error> {
    let (mut from_ssdp, mut from_mdns) = (Vec::new(), Vec::new());
    let ended = {
        let ssdp = search_ssdp(config, &until, &mut from_ssdp);
        let mdns = search_mdns(config, &until, &mut from_mdns);
        tokio::pin!(ssdp, mdns);
        let (mut ssdp_result, mut mdns_result) = (None, None);
        loop {
            tokio::select! {
                result = &mut ssdp, if ssdp_result.is_none() => ssdp_result = Some(result),
                result = &mut mdns, if mdns_result.is_none() => mdns_result = Some(result),
            }
            match (ssdp_result, mdns_result) {
                (Some(Ok(true)), _) | (_, Some(Ok(true))) => break None,
                (Some(ssdp), Some(mdns)) => break Some((ssdp, mdns)),
                (ssdp, mdns) => (ssdp_result, mdns_result) = (ssdp, mdns),
            }
        }
    };
    // `None`: a receiver matched.
    let Some((ssdp_result, mdns_result)) = ended else {
        return Ok(join(from_ssdp, from_mdns));
    };
    combine(
        ssdp_result.map(|_| {
            sort_by_address(&mut from_ssdp);
            from_ssdp
        }),
        // Two addresses of one receiver have the same serial number.
        mdns_result.map(|_| join(Vec::new(), from_mdns)),
    )
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
                || same_serial(
                    r.serial_number.as_deref(),
                    receiver.serial_number.as_deref(),
                )
        };
        if !all.iter().any(same) {
            all.push(receiver);
        }
    }
    sort_by_address(&mut all);
    all
}

/// True if both have the same serial number, and it has some text.
fn same_serial(first: Option<&str>, second: Option<&str>) -> bool {
    matches!((first.map(str::trim), second.map(str::trim)), (Some(a), Some(b)) if !a.is_empty() && a == b)
}

/// Browses until the wait is over, and adds each receiver to `receivers` as
/// soon as its description is read. Returns true if `until` matched a
/// receiver. The list is not sorted and can have one receiver at several
/// addresses.
async fn search_mdns(
    config: &DiscoveryConfig,
    until: &(dyn Fn(&DiscoveredReceiver) -> bool + Sync),
    receivers: &mut Vec<DiscoveredReceiver>,
) -> Result<bool, Error> {
    let daemon = Daemon(ServiceDaemon::new().map_err(|e| network_error("mDNS start", e))?);
    let events = daemon
        .0
        .browse(SERVICE_TYPE)
        .map_err(|e| network_error("mDNS browse", e))?;
    // Set to `None` at the end of the wait, which stops the browsing.
    let mut daemon = Some(daemon);
    let deadline = Instant::now() + config.wait;
    let mut found: HashMap<String, Announced> = HashMap::new();
    // Several names for one address need one request only.
    let mut addresses = HashSet::new();
    let mut fetches = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            event = tokio::time::timeout_at(deadline, events.recv_async()), if daemon.is_some() => {
                let Ok(Ok(event)) = event else {
                    tracing::debug!(count = found.len(), "mDNS search ended");
                    daemon = None;
                    continue;
                };
                match &event {
                    ServiceEvent::SearchStarted(kind) => tracing::debug!(kind, "mDNS search started"),
                    ServiceEvent::ServiceFound(_, name) => tracing::debug!(name, "mDNS service found"),
                    _ => {}
                }
                if let ServiceEvent::ServiceRemoved(_, name) = &event {
                    found.remove(name);
                }
                if let ServiceEvent::ServiceResolved(service) = event {
                    let name = &service.fullname;
                    store(&mut found, name, Announced::from_resolved(&service));
                    if let Some(announcement) = found.get(name)
                        && addresses.len() < MAX_SERVICES
                        && addresses.insert(announcement.address)
                    {
                        let announcement = announcement.clone();
                        let (http, probes) = (config.http, config.probes.clone());
                        fetches.spawn(async move {
                            receiver(&announcement, http, probes.as_deref()).await
                        });
                    }
                }
            }
            Some(result) = fetches.join_next() => {
                if let Ok(Some(receiver)) = result {
                    let matched = until(&receiver);
                    receivers.push(receiver);
                    if matched {
                        return Ok(true);
                    }
                }
            }
            else => break,
        }
    }
    // Only the services that are still announced.
    receivers.retain(|r| {
        r.host
            .parse::<IpAddr>()
            .is_ok_and(|host| found.values().any(|a| a.address == host))
    });
    Ok(false)
}

/// Keeps the answer for a service name. An answer without a usable address
/// replaces the old one with nothing.
fn store(found: &mut HashMap<String, Announced>, name: &str, announced: Option<Announced>) {
    match announced {
        Some(_) if found.len() >= MAX_SERVICES && !found.contains_key(name) => {
            tracing::debug!(name, "mDNS service ignored, too many");
        }
        Some(announced) => {
            tracing::debug!(name, address = %announced.address, "mDNS service resolved");
            found.insert(name.to_owned(), announced);
        }
        None => {
            found.remove(name);
            tracing::debug!(name, "mDNS service without a usable address");
        }
    }
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
    for location in DESCRIPTION_TYPES.map(location) {
        match describe(&location, http, probes).await {
            Described::Receiver(mut found) => {
                found.serial_number = found.serial_number.or_else(|| announced.did.clone());
                return Some(found);
            }
            Described::NotReceiver => return None,
            Described::Unusable => {}
        }
    }
    let identified = match probes {
        Some(probes) => identify_on(&host, probes, http).await,
        None => identify(&host, http).await,
    }
    .ok()?;
    if !is_proven_receiver(&identified) {
        return None;
    }
    // The brand from `BrandCode`, else from the model text. The model text has
    // the brand only on some receivers. Marantz needs its name.
    let brand = identified
        .device_info
        .brand
        .or_else(|| announced.model.as_deref().and_then(Brand::from_model))
        .unwrap_or(Brand::Denon);
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
        mac_address: identified
            .device_info
            .mac_address
            .map(std::borrow::Cow::into_owned),
        type_proof: identified.type_proof,
    })
}

/// Returns true if a device without a description is kept: the answers must
/// prove that it is an AV receiver. HEOS speakers announce the same service.
fn is_proven_receiver(identified: &Identified) -> bool {
    identified.type_proof.is_proven() && identified.device_info.is_av_receiver()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::identify::TypeProof;
    use denon_marantz_proto::DeviceInfo;

    fn receiver(host: &str, name: &str) -> DiscoveredReceiver {
        DiscoveredReceiver {
            host: host.to_owned(),
            brand: Brand::Denon,
            manufacturer: "Denon".to_owned(),
            model_name: None,
            friendly_name: Some(name.to_owned()),
            serial_number: None,
            mac_address: None,
            type_proof: TypeProof::Read,
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
        assert_eq!(names, [("192.168.1.3", "mdns"), ("192.168.1.20", "ssdp")]);
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
            sent: false,
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
    fn a_mapped_ipv6_address_is_used_as_ipv4() {
        let mapped = ["::ffff:192.168.1.9".parse::<IpAddr>().unwrap()];
        let announced = Announced::from_parts(mapped, |_| None).unwrap();
        assert_eq!(announced.address, "192.168.1.9".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn a_service_without_a_usable_address_is_left_out() {
        let addresses = ["fe80::1".parse::<IpAddr>().unwrap()];
        assert_eq!(Announced::from_parts(addresses, |_| None), None);
    }

    fn announced(address: &str) -> Announced {
        Announced {
            address: address.parse().unwrap(),
            model: None,
            did: None,
        }
    }

    #[test]
    fn a_blank_serial_number_does_not_join_receivers() {
        let mut first = receiver("192.168.1.20", "a");
        first.serial_number = Some(" ".to_owned());
        let mut second = receiver("192.168.1.3", "b");
        second.serial_number = Some(" ".to_owned());
        assert_eq!(join(vec![first], vec![second]).len(), 2);
    }

    #[test]
    fn two_addresses_with_one_serial_number_are_one_receiver() {
        let mut first = receiver("192.168.1.20", "a");
        first.serial_number = Some("S1".to_owned());
        let mut second = receiver("fd00::1", "b");
        second.serial_number = Some("S1".to_owned());
        let all = join(
            Vec::new(),
            vec![first, second, receiver("192.168.1.3", "c")],
        );
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn the_result_is_sorted_by_the_address_as_a_number() {
        let all = join(
            Vec::new(),
            vec![receiver("192.168.1.20", "a"), receiver("192.168.1.3", "b")],
        );
        assert_eq!(all[0].host, "192.168.1.3");
        assert_eq!(all[1].host, "192.168.1.20");
    }

    #[test]
    fn an_answer_without_an_address_removes_the_old_one() {
        let mut found = HashMap::new();
        store(&mut found, "a", Some(announced("192.168.1.3")));
        store(&mut found, "b", Some(announced("192.168.1.4")));
        store(&mut found, "a", None);
        assert_eq!(found.keys().collect::<Vec<_>>(), ["b"]);
    }

    #[test]
    fn a_new_name_is_ignored_at_the_limit_but_a_known_one_is_updated() {
        let mut found = HashMap::new();
        for index in 0..MAX_SERVICES {
            store(
                &mut found,
                &index.to_string(),
                Some(announced("192.168.1.3")),
            );
        }
        store(&mut found, "new", Some(announced("192.168.1.4")));
        assert_eq!(found.len(), MAX_SERVICES);
        assert!(!found.contains_key("new"));
        store(&mut found, "0", Some(announced("192.168.1.5")));
        assert_eq!(found["0"].address, "192.168.1.5".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn a_device_without_a_description_needs_proof() {
        let identified = |type_proof, category: Option<&'static str>| Identified {
            receiver_type: ReceiverType::Avr,
            port: 80,
            device_info: DeviceInfo {
                category: category.map(Into::into),
                ..DeviceInfo::default()
            },
            type_proof,
        };
        assert!(is_proven_receiver(&identified(TypeProof::Read, None)));
        assert!(is_proven_receiver(&identified(
            TypeProof::OldModelPage,
            None
        )));
        assert!(!is_proven_receiver(&identified(TypeProof::Failed, None)));
        assert!(!is_proven_receiver(&identified(
            TypeProof::Read,
            Some("SOUND BAR")
        )));
    }
}
