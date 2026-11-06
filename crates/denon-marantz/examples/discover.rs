//! Looks for receivers with SSDP and prints each one.
//!
//! ```sh
//! cargo run -p denon-marantz --example discover
//! cargo run -p denon-marantz --example discover -- --same-host
//! cargo run -p denon-marantz --example discover -- --from 192.168.1.4
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
//! - `--wait SECONDS`: time to wait for answers, default 2.5.
//!
//! The output has the serial number of each receiver.

use std::net::Ipv4Addr;
use std::time::Duration;

use denon_marantz::{DiscoveryConfig, LocationPolicy, discover};
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let mut config = DiscoveryConfig::default();
    let mut args = std::env::args().skip(1);
    while let Some(option) = args.next() {
        match option.as_str() {
            "--same-host" => config.locations = LocationPolicy::SameHost,
            "--from" => {
                let address: Ipv4Addr = args
                    .next()
                    .and_then(|value| value.parse().ok())
                    .ok_or("--from needs an IPv4 address")?;
                config.interfaces = Some(vec![address]);
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
    let receivers = discover(&config).await?;
    println!(
        "{} receiver(s) in {:.1?}",
        receivers.len(),
        started.elapsed()
    );
    for receiver in receivers {
        println!(
            "{:<16} {} | {} | {} | {}",
            receiver.host,
            receiver.brand.name(),
            receiver.model_name.as_deref().unwrap_or("-"),
            receiver.friendly_name.as_deref().unwrap_or("-"),
            receiver.serial_number.as_deref().unwrap_or("-"),
        );
    }
    Ok(())
}
