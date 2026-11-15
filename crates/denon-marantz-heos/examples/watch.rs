//! Connects to a HEOS device and prints what changes, until you press Ctrl-C.
//!
//! Play, pause or skip in the HEOS app, and watch the lines that follow.
//!
//! ```sh
//! cargo run -p denon-marantz-heos --example watch -- 192.168.1.20
//! RUST_LOG=denon_marantz_heos=debug cargo run -p denon-marantz-heos --example watch -- 192.168.1.20
//! ```
//!
//! The output can have personal data, for example a title that plays.

use denon_marantz_heos::{Change, Heos};
use tokio::sync::broadcast::error::RecvError;
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let host = std::env::args()
        .nth(1)
        .ok_or("usage: watch <host or IP address of the HEOS device>")?;

    let heos = Heos::connect(host).await?;
    let mut changes = heos.changes();
    for state in heos.players() {
        println!(
            "player  {} {}: {:?} {:?}",
            state.pid(),
            state.player.name,
            state.play_state,
            state.current_media()
        );
    }
    println!("connected, press Ctrl-C to stop");

    loop {
        tokio::select! {
            change = changes.recv() => match change {
                Ok(change) => print_change(&heos, &change),
                Err(RecvError::Lagged(count)) => println!("lost {count} changes"),
                Err(RecvError::Closed) => break,
            },
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    heos.close().await;
    Ok(())
}

fn print_change(heos: &Heos, change: &Change) {
    let state = match change {
        Change::PlayState(pid) | Change::NowPlaying(pid) | Change::Progress(pid) => {
            heos.player(*pid)
        }
        _ => None,
    };
    match (change, state) {
        (Change::PlayState(_), Some(state)) => println!("state   {:?}", state.play_state),
        (Change::NowPlaying(_), Some(state)) => {
            println!("media   {:?} {:?}", state.current_media(), state.controls());
        }
        (Change::Progress(_), Some(state)) => println!("time    {:?}", state.progress),
        _ => println!("change  {change:?}"),
    }
}
