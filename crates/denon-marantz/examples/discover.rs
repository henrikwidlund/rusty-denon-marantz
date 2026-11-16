//! Looks for receivers with SSDP and prints each one.
//!
//! ```sh
//! cargo run -p denon-marantz --example discover
//! cargo run -p denon-marantz --example discover -- --same-host
//! cargo run -p denon-marantz --example discover -- --from 192.168.1.4
//! cargo run -p denon-marantz --example discover -- --target 192.168.1.10:1900
//! cargo run -p denon-marantz --features mdns --example discover -- --mdns
//! cargo run -p denon-marantz --features mdns --example discover -- --mdns-only
//! RUST_LOG=denon_marantz=debug cargo run -p denon-marantz --example discover
//! ```
//!
//! Options:
//!
//! - `--same-host`: use only a location that has the address that the answer
//!   came from. A router that relays SSDP answers from its own address breaks
//!   this.
//! - `--from ADDRESS`: search from this local address only. The default is every
//!   IPv4 address of the computer.
//! - `--target ADDRESS:PORT`: send the searches to this address and not to the
//!   multicast address, for example `192.168.1.10:1900`. A receiver answers a
//!   search that is sent to it. This needs no multicast.
//! - `--mdns`: search with mDNS too, and join the answers. Needs the `mdns`
//!   feature.
//! - `--mdns-only`: search with mDNS only. Needs the `mdns` feature.
//! - `--wait SECONDS`: time to wait for answers, default 2.5.
//!
//! The output has the serial number and the MAC address of each receiver.

use std::net::Ipv4Addr;
use std::time::Duration;

use denon_marantz::{DiscoveryConfig, LocationPolicy, discover};
use tracing_subscriber::EnvFilter;

/// Which searches to run.
#[cfg(feature = "mdns")]
enum Mode {
    Ssdp,
    Both,
    MdnsOnly,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let mut config = DiscoveryConfig::default();
    #[cfg(feature = "mdns")]
    let mut mode = Mode::Ssdp;
    let mut args = std::env::args().skip(1);
    while let Some(option) = args.next() {
        match option.as_str() {
            #[cfg(feature = "mdns")]
            "--mdns" => mode = Mode::Both,
            #[cfg(feature = "mdns")]
            "--mdns-only" => mode = Mode::MdnsOnly,
            "--same-host" => config.locations = LocationPolicy::SameHost,
            "--from" => {
                let address: Ipv4Addr = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or("--from needs an IPv4 address")?;
                config.interfaces = Some(vec![address]);
            }
            "--target" => {
                config.target = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or("--target needs an address and a port, for example 192.168.1.10:1900")?;
            }
            "--wait" => {
                let seconds: f64 = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or("--wait needs a number of seconds")?;
                config.wait = Duration::try_from_secs_f64(seconds)?;
            }
            other => return Err(format!("unknown option {other}").into()),
        }
    }

    let started = std::time::Instant::now();
    #[cfg(feature = "mdns")]
    let search = async {
        match mode {
            Mode::Ssdp => discover(&config).await,
            Mode::Both => denon_marantz::discover_all(&config).await,
            Mode::MdnsOnly => denon_marantz::discover_mdns(&config).await,
        }
    };
    #[cfg(not(feature = "mdns"))]
    let search = discover(&config);
    let receivers = search.await.inspect_err(|error| {
        if error.is_network() {
            eprintln!(
                "Hint: on macOS, give the Local Network permission to this terminal, or use --target."
            );
        }
    })?;
    println!(
        "{} receiver(s) in {:.1?}",
        receivers.len(),
        started.elapsed()
    );
    for receiver in receivers {
        println!(
            "{:<16} {} | {} | {} | {} | {}",
            receiver.host,
            receiver.brand.name(),
            receiver.model_name.as_deref().unwrap_or("-"),
            receiver.friendly_name.as_deref().unwrap_or("-"),
            receiver.serial_number.as_deref().unwrap_or("-"),
            receiver.mac_address.as_deref().unwrap_or("-"),
        );
    }
    Ok(())
}
