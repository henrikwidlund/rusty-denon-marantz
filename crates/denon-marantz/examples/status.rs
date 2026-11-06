//! Connects to a receiver and prints what it tells about itself and its state.
//!
//! The output has the serial number of the receiver. Remove it before you
//! share the output.
//!
//! ```sh
//! cargo run -p denon-marantz --example status -- 192.168.1.20
//! RUST_LOG=denon_marantz=debug cargo run -p denon-marantz --example status -- 192.168.1.20
//! ```

use denon_marantz::Avr;
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let host = std::env::args()
        .nth(1)
        .ok_or("usage: status <host or IP address of the receiver>")?;

    let avr = Avr::builder(host).connect().await?;
    println!("{:#?}", avr.info());
    avr.update().await?;
    println!("{:#?}", avr.snapshot());
    Ok(())
}
