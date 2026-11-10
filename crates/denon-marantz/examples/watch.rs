//! Connects to a receiver and prints what changes, until you press Ctrl-C.
//!
//! Change the volume, the source or the power on the receiver or with its
//! remote control, and watch the lines that follow.
//!
//! ```sh
//! cargo run -p denon-marantz --example watch -- 192.168.1.20
//! RUST_LOG=denon_marantz=debug cargo run -p denon-marantz --example watch -- 192.168.1.20
//! ```
//!
//! The events can have personal data, for example a title that plays.

use denon_marantz::Avr;
use tokio::sync::broadcast::error::RecvError;
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let host = std::env::args()
        .nth(1)
        .ok_or("usage: watch <host or IP address of the receiver>")?;

    let avr = Avr::builder(host).connect().await?;
    println!("connected to {}", avr.info().friendly_name);
    let mut events = avr.telnet_events();
    let mut changes = avr.changes();
    avr.update().await?;
    avr.telnet_connect().await?;
    println!("telnet is open, press Ctrl-C to stop");

    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => println!("event   {:?} {:?} {:?}", event.zone, event.event, event.parameter),
                Err(RecvError::Lagged(count)) => println!("lost {count} events"),
                Err(RecvError::Closed) => break,
            },
            change = changes.recv() => match change {
                Ok(change) => {
                    let state = avr.snapshot();
                    match change.zone {
                        Some(zone) => println!("change  {change:?} -> {:?}", state.zone(zone)),
                        None => println!("change  {change:?} -> {:?}", state.device()),
                    }
                }
                Err(RecvError::Lagged(count)) => println!("lost {count} changes"),
                Err(RecvError::Closed) => break,
            },
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    avr.telnet_disconnect().await;
    Ok(())
}
