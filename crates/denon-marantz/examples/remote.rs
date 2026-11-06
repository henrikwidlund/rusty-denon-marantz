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
//! tone-on, tone-off, bass DB, bass-up, bass-down, treble DB, treble-up,
//! treble-down (DB is a whole number from -6 to 6), dynamic-eq-on,
//! dynamic-eq-off, multi-eq CODE, ref-level CODE, dynamic-volume CODE, lfc-on,
//! lfc-off, containment 1-7, containment-up, containment-down, dirac CODE dirac-name NAME,
//! room-size CODE, audio-restorer CODE, dialog-enhancer CODE, graphic-eq on or off (a
//! CODE is the telnet code, for example AUDYSSEY, 5, HEV, ML or OFF), volume-up, volume-down, volume DB, mute, unmute, source NAME, sound-mode NAME,
//! all-zone-stereo-on, all-zone-stereo-off, play, pause, stop, up, down, left,
//! right, enter, back, info, options, setup-open, setup-close, raw TEXT.
//!
//! The events can have personal data, for example a title that plays.

use std::time::Duration;

use denon_marantz::{Avr, Command, TelnetConfig, ToneControl, ZoneControl};
use denon_marantz_proto::{
    AudioRestorer, DialogEnhancerLevel, DiracFilter, DynamicVolume, MultiEq, ReferenceLevelOffset,
    RoomSize, Zone,
};
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

/// Commands that the tone control has methods for.
const TONE_COMMANDS: [&str; 8] = [
    "tone-on",
    "tone-off",
    "bass",
    "bass-up",
    "bass-down",
    "treble",
    "treble-up",
    "treble-down",
];

/// Commands that the Audyssey, Dirac and sound controls have methods for.
const SOUND_COMMANDS: [&str; 16] = [
    "dynamic-eq-on",
    "dynamic-eq-off",
    "multi-eq",
    "ref-level",
    "dynamic-volume",
    "lfc-on",
    "lfc-off",
    "containment",
    "containment-up",
    "containment-down",
    "dirac",
    "dirac-name",
    "room-size",
    "audio-restorer",
    "dialog-enhancer",
    "graphic-eq",
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

/// Runs a command with the methods of the tone control.
async fn tone_command(
    tone: ToneControl<'_>,
    name: &str,
    argument: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let db = || -> Result<i8, String> {
        argument
            .ok_or_else(|| format!("{name} needs an argument"))?
            .parse()
            .map_err(|_| format!("{name} is a whole number of dB"))
    };
    match name {
        "tone-on" => tone.set_enabled(true).await?,
        "tone-off" => tone.set_enabled(false).await?,
        "bass" => tone.set_bass_db(db()?).await?,
        "bass-up" => tone.bass_up().await?,
        "bass-down" => tone.bass_down().await?,
        "treble" => tone.set_treble_db(db()?).await?,
        "treble-up" => tone.treble_up().await?,
        "treble-down" => tone.treble_down().await?,
        other => return Err(format!("unknown command {other}").into()),
    }
    Ok(())
}

/// Runs a command with the methods of the Audyssey and Dirac controls. The
/// argument is the telnet code, for example `AUDYSSEY` or `HEV`.
async fn sound_command(
    avr: &Avr,
    name: &str,
    argument: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let need = || argument.ok_or_else(|| format!("{name} needs an argument"));
    let unknown = |what: &str| format!("{what} is not known: {}", argument.unwrap_or_default());
    let audyssey = avr.audyssey();
    match name {
        "dynamic-eq-on" => audyssey.set_dynamic_eq(true).await?,
        "dynamic-eq-off" => audyssey.set_dynamic_eq(false).await?,
        "multi-eq" => {
            let mode = MultiEq::from_telnet(need()?).ok_or_else(|| unknown("MultEQ mode"))?;
            audyssey.set_multi_eq(mode).await?;
        }
        "ref-level" => {
            let offset = ReferenceLevelOffset::from_telnet(need()?)
                .ok_or_else(|| unknown("reference level offset"))?;
            audyssey.set_reference_level_offset(offset).await?;
        }
        "dynamic-volume" => {
            let volume =
                DynamicVolume::from_telnet(need()?).ok_or_else(|| unknown("Dynamic Volume"))?;
            audyssey.set_dynamic_volume(volume).await?;
        }
        "lfc-on" => audyssey.set_lfc(true).await?,
        "lfc-off" => audyssey.set_lfc(false).await?,
        "containment" => {
            let level = need()?.parse().map_err(|_| "containment is from 1 to 7")?;
            audyssey.set_containment_level(level).await?;
        }
        "containment-up" => audyssey.containment_amount_up().await?,
        "containment-down" => audyssey.containment_amount_down().await?,
        "dirac" => {
            let filter = DiracFilter::from_code(need()?).ok_or_else(|| unknown("Dirac filter"))?;
            avr.dirac()?.set_filter(filter).await?;
        }
        "dirac-name" => avr.dirac()?.set_slot_by_name(need()?).await?,
        "room-size" => {
            let size = RoomSize::from_code(need()?).ok_or_else(|| unknown("room size"))?;
            avr.sound().set_room_size(size).await?;
        }
        "audio-restorer" => {
            let level =
                AudioRestorer::from_code(need()?).ok_or_else(|| unknown("audio restorer"))?;
            avr.sound().set_audio_restorer(level).await?;
        }
        "dialog-enhancer" => {
            let level = DialogEnhancerLevel::from_code(need()?)
                .ok_or_else(|| unknown("dialog enhancer"))?;
            avr.sound().set_dialog_enhancer(level).await?;
        }
        "graphic-eq" => {
            let on = match need()? {
                "on" => true,
                "off" => false,
                _ => return Err(unknown("graphic EQ").into()),
            };
            avr.sound().set_graphic_eq(on).await?;
        }
        other => return Err(format!("unknown command {other}").into()),
    }
    Ok(())
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
    let typed = |commands: &[&str]| commands.contains(&name.as_str());
    let other = if typed(&ZONE_COMMANDS) || typed(&TONE_COMMANDS) || typed(&SOUND_COMMANDS) {
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
        None if typed(&SOUND_COMMANDS) => {
            sound_command(&avr, &name, argument.as_deref()).await?;
        }
        None if typed(&TONE_COMMANDS) => {
            tone_command(avr.tone(), &name, argument.as_deref()).await?;
        }
        None => zone_command(avr.zone(zone)?, &name, argument.as_deref()).await?,
    }
    println!("[{:7.3}] written", stamp());
    sleep(listen).await;

    printer.abort();
    avr.telnet_disconnect().await;
    Ok(())
}
