//! Finding out which kind of receiver answers on a host.

use denon_marantz_proto::{DEVICE_INFO_PATH, DeviceInfo, ReceiverType};

use crate::error::Error;
use crate::http::{HttpClient, HttpConfig};

/// What `Deviceinfo.xml` tells about a receiver.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identified {
    /// Generation of the receiver.
    pub receiver_type: ReceiverType,
    /// Port of the web interface.
    pub port: u16,
    /// Values from the device information. Empty if the receiver has none.
    pub device_info: DeviceInfo<'static>,
}

/// Ports to try, newest generation first.
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
/// Returns the last error if the receiver does not answer on any port.
pub async fn identify(host: &str, config: HttpConfig) -> Result<Identified, Error> {
    identify_on(host, &CANDIDATES, config).await
}

/// Like [`identify`], with your own list of types and ports to try.
///
/// # Errors
///
/// Returns the last error if the receiver does not answer on any port.
pub async fn identify_on(
    host: &str,
    candidates: &[(ReceiverType, u16)],
    config: HttpConfig,
) -> Result<Identified, Error> {
    let mut device_info = None;
    let mut last_error = None;
    let mut unreachable = 0;
    for &(receiver_type, port) in candidates {
        let client = HttpClient::new(host, port, config)?;
        let info = match client
            .get_parsed(DEVICE_INFO_PATH, |body| {
                DeviceInfo::parse(body).map(DeviceInfo::into_owned)
            })
            .await
        {
            Ok(info) => info,
            Err(error) if error.is_timeout() || error.is_network() => {
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
        device_info = Some(info);
    }
    if unreachable == candidates.len()
        && let Some(error) = last_error
    {
        return Err(error);
    }
    tracing::info!(host, "identified avr receiver");
    Ok(Identified {
        receiver_type: ReceiverType::Avr,
        port: ReceiverType::Avr.port(),
        device_info: device_info.unwrap_or_default(),
    })
}
