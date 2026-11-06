//! Sends one command to a receiver and prints the events that follow.
//!
//! ```sh
//! cargo run -p denon-marantz --example remote -- 192.168.1.20 volume-up
//! cargo run -p denon-marantz --example remote -- 192.168.1.20 volume -40.5
//! cargo run -p denon-marantz --example remote -- 192.168.1.20 --zone 2 power-on
//! cargo run -p denon-marantz --example remote -- 192.168.1.20 raw "MV?"
//! RUST_LOG=denon_marantz=debug cargo run -p denon-marantz --example remote -- 192.168.1.20 mute
//! ```
//!
//! Commands: power-on, power-standby, device-power-on, device-power-standby,
//! volume-up, volume-down, volume DB, mute, unmute, source NAME, sound-mode NAME,
//! all-zone-stereo-on, all-zone-stereo-off, play, pause, stop, up, down, left,
//! right, enter, back, info, options, setup-open, setup-close, raw TEXT.
//!
//! The events can have personal data, for example a title that plays.

use std::time::Duration;

use denon_marantz::{Avr, Command};
use denon_marantz_proto::{Volume, Zone};
use tokio::sync::broadcast::error::RecvError;
use tracing_subscriber::EnvFilter;

/// How long the events are shown after the command.
const LISTEN: Duration = Duration::from_secs(3);

const USAGE: &str =
    "usage: remote <host> [--zone 2|3] <command> [argument]  (see the top of the example file)";

fn command<'a>(name: &str, argument: Option<&'a str>) -> Result<Command<'a>, String> {
    let need = || argument.ok_or_else(|| format!("{name} needs an argument"));
    Ok(match name {
        "power-on" => Command::PowerOn,
        "power-standby" => Command::PowerStandby,
        "device-power-on" => Command::DevicePowerOn,
        "device-power-standby" => Command::DevicePowerStandby,
        "volume-up" => Command::VolumeUp,
        "volume-down" => Command::VolumeDown,
        "volume" => {
            let db: f32 = need()?.parse().map_err(|_| "volume is a number in dB")?;
            Command::SetVolume(Volume::from_db_rounded(db).ok_or("volume is from -80 to 18 dB")?)
        }
        "mute" => Command::Mute(true),
        "unmute" => Command::Mute(false),
        "source" => Command::SelectSource(need()?),
        "sound-mode" => Command::SelectSoundMode(need()?),
        "all-zone-stereo-on" => Command::AllZoneStereo(true),
        "all-zone-stereo-off" => Command::AllZoneStereo(false),
        "play" => Command::Play,
        "pause" => Command::Pause,
        "stop" => Command::Stop,
        "up" => Command::CursorUp,
        "down" => Command::CursorDown,
        "left" => Command::CursorLeft,
        "right" => Command::CursorRight,
        "enter" => Command::CursorEnter,
        "back" => Command::Back,
        "info" => Command::Info,
        "options" => Command::Options,
        "setup-open" => Command::SetupMenu(true),
        "setup-close" => Command::SetupMenu(false),
        "raw" => Command::Raw(need()?),
        other => return Err(format!("unknown command {other}")),
    })
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let mut args = std::env::args().skip(1);
    let host = args.next().ok_or(USAGE)?;
    let mut name = args.next().ok_or(USAGE)?;
    let mut zone = Zone::Main;
    if name == "--zone" {
        zone = match args.next().as_deref() {
            Some("2") => Zone::Zone2,
            Some("3") => Zone::Zone3,
            _ => return Err("--zone is 2 or 3".into()),
        };
        name = args.next().ok_or(USAGE)?;
    }
    let argument = args.next();
    let command = command(&name, argument.as_deref())?;

    let avr = Avr::builder(host).connect().await?;
    println!("connected to {}", avr.info().friendly_name);
    let mut events = avr.events();
    avr.telnet_connect().await?;
    avr.send(zone, command).await?;
    println!("sent {name} to {zone:?}, events for {LISTEN:?}:");

    let end = tokio::time::sleep(LISTEN);
    tokio::pin!(end);
    loop {
        tokio::select! {
            event = events.recv() => match event {
                Ok(event) => println!("event   {:?} {:?} {:?}", event.zone, event.event, event.parameter),
                Err(RecvError::Lagged(count)) => println!("lost {count} events"),
                Err(RecvError::Closed) => break,
            },
            () = &mut end => break,
        }
    }
    avr.telnet_disconnect().await;
    Ok(())
}
