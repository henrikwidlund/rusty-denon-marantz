//! Sends one command to a receiver and prints the events, with the time of each.
//!
//! ```sh
//! cargo run -p denon-marantz --example remote -- 192.168.1.20 volume-up
//! cargo run -p denon-marantz --example remote -- 192.168.1.20 volume -40.5
//! cargo run -p denon-marantz --example remote -- 192.168.1.20 --zone 2 power-on
//! cargo run -p denon-marantz --example remote -- 192.168.1.20 raw "MV?"
//! cargo run -p denon-marantz --example remote -- 192.168.1.20 --wait 15 --listen 2 mute
//! RUST_LOG=denon_marantz=debug cargo run -p denon-marantz --example remote -- 192.168.1.20 mute
//! ```
//!
//! Options before the command:
//!
//! - `--zone 2` or `--zone 3`: the zone of the command, default is the main zone.
//! - `--wait SECONDS`: wait after the connect before the command is sent. The
//!   receiver answers the queries after a connect for several seconds. With a
//!   wait you see how fast it answers a command when it is not busy.
//! - `--listen SECONDS`: how long to show events after the command, default 3.
//! - `--command-interval MS`, `--query-interval MS` and `--query-quiet MS`: the
//!   spacing of writes. A query also waits until the receiver sent no data for
//!   the quiet time.
//!
//! Commands: power-on, power-standby, device-power-on, device-power-standby,
//! volume-up, volume-down, volume DB, mute, unmute, source NAME, sound-mode NAME,
//! all-zone-stereo-on, all-zone-stereo-off, play, pause, stop, up, down, left,
//! right, enter, back, info, options, setup-open, setup-close, raw TEXT.
//!
//! The events can have personal data, for example a title that plays.

use std::time::Duration;

use denon_marantz::{Avr, Command, TelnetConfig, ZoneControl};
use denon_marantz_proto::Zone;
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{Instant, sleep};
use tracing_subscriber::EnvFilter;

const USAGE: &str =
    "usage: remote <host> [options] <command> [argument]  (see the top of the example file)";

/// Commands that the zone control has methods for.
const ZONE_COMMANDS: [&str; 9] = [
    "power-on",
    "power-standby",
    "volume-up",
    "volume-down",
    "volume",
    "mute",
    "unmute",
    "source",
    "sound-mode",
];

/// The other commands, sent with `Avr::send`.
fn command<'a>(name: &str, argument: Option<&'a str>) -> Result<Command<'a>, String> {
    Ok(match name {
        "device-power-on" => Command::DevicePowerOn,
        "device-power-standby" => Command::DevicePowerStandby,
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
        "raw" => Command::Raw(argument.ok_or("raw needs an argument")?),
        other => return Err(format!("unknown command {other}")),
    })
}

/// Runs a command with the methods of the zone control.
async fn zone_command(
    zone: ZoneControl<'_>,
    name: &str,
    argument: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let need = || argument.ok_or_else(|| format!("{name} needs an argument"));
    match name {
        "power-on" => zone.power_on().await?,
        "power-standby" => zone.power_standby().await?,
        "volume-up" => zone.volume_up().await?,
        "volume-down" => zone.volume_down().await?,
        "volume" => {
            let db: f32 = need()?.parse().map_err(|_| "volume is a number in dB")?;
            zone.set_volume_db(db).await?;
        }
        "mute" => zone.set_mute(true).await?,
        "unmute" => zone.set_mute(false).await?,
        "source" => zone.select_source(need()?).await?,
        "sound-mode" => zone.select_sound_mode(need()?).await?,
        other => return Err(format!("unknown command {other}").into()),
    }
    Ok(())
}

/// The next argument as a number.
fn number(args: &mut impl Iterator<Item = String>, option: &str) -> Result<u64, String> {
    args.next()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("{option} needs a whole number"))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let mut args = std::env::args().skip(1);
    let host = args.next().ok_or(USAGE)?;
    let mut zone = Zone::Main;
    let (mut wait, mut listen) = (Duration::ZERO, Duration::from_secs(3));
    let mut config = TelnetConfig::default();
    let name = loop {
        let word = args.next().ok_or(USAGE)?;
        match word.as_str() {
            "--zone" => {
                zone = match args.next().as_deref() {
                    Some("2") => Zone::Zone2,
                    Some("3") => Zone::Zone3,
                    _ => return Err("--zone is 2 or 3".into()),
                };
            }
            "--wait" => wait = Duration::from_secs(number(&mut args, "--wait")?),
            "--listen" => listen = Duration::from_secs(number(&mut args, "--listen")?),
            "--command-interval" => {
                config.command_interval =
                    Duration::from_millis(number(&mut args, "--command-interval")?);
            }
            "--query-interval" => {
                config.query_interval =
                    Duration::from_millis(number(&mut args, "--query-interval")?);
            }
            "--query-quiet" => {
                config.query_quiet = Duration::from_millis(number(&mut args, "--query-quiet")?);
            }
            _ => break word,
        }
    };
    let argument = args.next();
    // A command that is not known is refused before the connect.
    let other = if ZONE_COMMANDS.contains(&name.as_str()) {
        None
    } else {
        Some(command(&name, argument.as_deref())?)
    };

    let avr = Avr::builder(host).telnet_config(config).connect().await?;
    println!("connected to {}", avr.info().friendly_name);
    let start = Instant::now();
    let stamp = move || start.elapsed().as_secs_f64();

    let mut events = avr.events();
    avr.telnet_connect().await?;
    let printer = tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(event) => println!(
                    "[{:7.3}] event   {:?} {:?} {:?}",
                    stamp(),
                    event.zone,
                    event.event,
                    event.parameter
                ),
                Err(RecvError::Lagged(count)) => println!("[{:7.3}] lost {count} events", stamp()),
                Err(RecvError::Closed) => break,
            }
        }
    });

    sleep(wait).await;
    println!("[{:7.3}] sending {name} to {zone:?}", stamp());
    match other {
        Some(command) => avr.send(zone, command).await?,
        None => zone_command(avr.zone(zone)?, &name, argument.as_deref()).await?,
    }
    println!("[{:7.3}] written", stamp());
    sleep(listen).await;

    printer.abort();
    avr.telnet_disconnect().await;
    Ok(())
}
