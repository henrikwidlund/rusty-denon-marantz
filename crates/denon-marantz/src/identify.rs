//! Finding out which kind of receiver answers on a host.

use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::Poll;

use denon_marantz_proto::{DEVICE_INFO_PATH, DeviceInfo, ReceiverType};

use crate::error::Error;
use crate::http::{HttpClient, HttpConfig};

/// What `Deviceinfo.xml` tells about a receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identified {
    /// Generation of the receiver.
    pub receiver_type: ReceiverType,
    /// HTTP port that is used for AppCommand and the status pages.
    pub port: u16,
    /// Values from the device information. Empty if the receiver has none.
    pub device_info: DeviceInfo<'static>,
}

/// Ports to try. For types on the same port, the first one is used.
const CANDIDATES: [(ReceiverType, u16); 3] = [
    (ReceiverType::AvrX2016, 8080),
    (ReceiverType::AvrX, 80),
    (ReceiverType::Avr, 80),
];

/// Finds the type of the receiver by asking for `Deviceinfo.xml`.
///
/// Newer receivers answer on port 8080, older AVR-X receivers on port 80.
/// Receivers that are not AVR-X give an error page or no usable XML. They are
/// reported as [`ReceiverType::Avr`] on port 80.
///
/// # Errors
///
/// Returns the error of the port that failed last if the receiver does not
/// answer on any port.
pub async fn identify(host: &str, config: HttpConfig) -> Result<Identified, Error> {
    identify_on(host, &CANDIDATES, config).await
}

type Answer = (u16, Result<DeviceInfo<'static>, Error>);

/// Like [`identify`], with your own list of types and ports to try.
///
/// All ports are asked at the same time, each port once. The first AVR-X
/// answer wins. If several types use the same port, the first of them in the
/// list is used.
///
/// # Errors
///
/// Returns the error of the port that failed last if the receiver does not
/// answer on any port.
pub async fn identify_on(
    host: &str,
    candidates: &[(ReceiverType, u16)],
    config: HttpConfig,
) -> Result<Identified, Error> {
    let mut pending: Vec<Pin<Box<dyn Future<Output = Answer> + Send + '_>>> = Vec::new();
    for (index, &(_, port)) in candidates.iter().enumerate() {
        if candidates[..index].iter().any(|&(_, p)| p == port) {
            continue;
        }
        let client = HttpClient::new(host, port, config)?;
        pending.push(Box::pin(async move {
            let info = client
                .get_parsed(DEVICE_INFO_PATH, |body| {
                    DeviceInfo::parse(body).map(DeviceInfo::into_owned)
                })
                .await;
            (port, info)
        }));
    }
    // The port that was tried for this type, or the usual one.
    let avr_port = candidates
        .iter()
        .find(|(receiver_type, _)| *receiver_type == ReceiverType::Avr)
        .map_or(ReceiverType::Avr.port(), |&(_, port)| port);
    let ports = pending.len();
    let mut device_info = None;
    let mut last_error = None;
    let mut unreachable = 0;
    while !pending.is_empty() {
        let (port, info) = poll_fn(|cx| {
            let ready = pending.iter_mut().enumerate().find_map(|(index, answer)| {
                match answer.as_mut().poll(cx) {
                    Poll::Ready(answer) => Some((index, answer)),
                    Poll::Pending => None,
                }
            });
            ready.map_or(Poll::Pending, |(index, answer)| {
                drop(pending.swap_remove(index));
                Poll::Ready(answer)
            })
        })
        .await;
        let receiver_type = candidates
            .iter()
            .find(|&&(_, p)| p == port)
            .map_or(ReceiverType::Avr, |&(receiver_type, _)| receiver_type);
        let info = match info {
            Ok(info) => info,
            Err(error) if error.is_unreachable() => {
                tracing::debug!(host, port, %error, "no answer when identifying the receiver");
                unreachable += 1;
                last_error = Some(error);
                continue;
            }
            Err(error) => {
                tracing::debug!(host, port, %error, "not a {} receiver", receiver_type.name());
                continue;
            }
        };
        if info.is_avr_x() {
            tracing::info!(host, port, "identified {} receiver", receiver_type.name());
            return Ok(Identified {
                receiver_type,
                port,
                device_info: info,
            });
        }
        if port == avr_port || device_info.is_none() {
            device_info = Some(info);
        }
    }
    if unreachable == ports
        && let Some(error) = last_error
    {
        return Err(error);
    }
    tracing::info!(host, "identified avr receiver");
    Ok(Identified {
        receiver_type: ReceiverType::Avr,
        port: avr_port,
        device_info: device_info.unwrap_or_default(),
    })
}
