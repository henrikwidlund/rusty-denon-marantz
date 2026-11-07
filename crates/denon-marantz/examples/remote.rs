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
//! room-size CODE, audio-restorer CODE, dialog-enhancer CODE, graphic-eq on or off, input-mode CODE (AUTO, HDMI, DIGITAL or ANALOG),
//! video-mode CODE (AUTO, GAME, MOVI, BYP), hdmi-output CODE (MONIAUTO, MONI1, MONI2),
//! hdmi-audio CODE (AMP or TV), aspect CODE (NRM or FUL), vertical-stretch on or off,
//! resolution CODE (AUTO, 48P, 10I, 72P, 10P, 10P24, 4K, 4KF, 8K), upscaler CODE (OFF or a resolution code) (a
//! CODE is the telnet code, for example AUDYSSEY, 5, HEV, ML or OFF), volume-up, volume-down, volume DB, mute, unmute, source NAME, sound-mode NAME,
//! all-zone-stereo-on, all-zone-stereo-off, play, pause, stop, up, down, left,
//! right, enter, back, info, options, setup-open, setup-close, trigger-on N, trigger-off N (N is 1 to 3), raw TEXT,
//! key NAME (next, previous, repeat-off, repeat-one, repeat-all, random-on,
//! random-off, page-up, page-down, rewind, fast-forward, atmos, cec-on,
//! cec-off, digit-0 to digit-9, volume-0db, volume-minus-20db,
//! volume-minus-40db), denon-code NUMBER, marantz-code DIGITS.
//!
//! The events can have personal data, for example a title that plays.

use std::time::Duration;

use denon_marantz::{Avr, Command, RemoteKey, TelnetConfig, ToneControl, ZoneControl};
use denon_marantz_proto::{
    AspectRatio, AudioRestorer, DialogEnhancerLevel, DiracFilter, DynamicVolume, HdmiAudioDecode,
    HdmiOutput, HdmiUpscaler, InputMode, MultiEq, ReferenceLevelOffset, RoomSize, Trigger,
    VideoProcessingMode, VideoResolution, Zone,
};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{Instant, sleep};
use tracing_subscriber::EnvFilter;

const USAGE: &str =
    "usage: remote <host> [options] <command> [argument]  (see the top of the example file)";

/// Commands that the zone control has methods for.
const ZONE_COMMANDS: [&str; 12] = [
    "power-on",
    "power-standby",
    "volume-up",
    "volume-down",
    "volume",
    "mute",
    "unmute",
    "source",
    "sound-mode",
    "key",
    "denon-code",
    "marantz-code",
];

/// Commands that the video control has methods for.
const VIDEO_COMMANDS: [&str; 7] = [
    "video-mode",
    "hdmi-output",
    "hdmi-audio",
    "aspect",
    "vertical-stretch",
    "resolution",
    "upscaler",
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
const SOUND_COMMANDS: [&str; 17] = [
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
    "input-mode",
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
        "trigger-on" | "trigger-off" => {
            let number: u8 = argument
                .and_then(|text| text.parse().ok())
                .ok_or("trigger needs a number from 1 to 3")?;
            let trigger = Trigger::from_number(number).ok_or("trigger is from 1 to 3")?;
            Command::SetTrigger(trigger, name == "trigger-on")
        }
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
        "input-mode" => {
            let mode = InputMode::from_code(need()?).ok_or_else(|| unknown("input mode"))?;
            avr.sound().set_input_mode(mode).await?;
        }
        other => return Err(format!("unknown command {other}").into()),
    }
    Ok(())
}

/// Runs a command with the methods of the video control.
async fn video_command(
    avr: &Avr,
    name: &str,
    argument: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let need = || argument.ok_or_else(|| format!("{name} needs an argument"));
    let unknown = |what: &str| format!("{what} is not known: {}", argument.unwrap_or_default());
    let video = avr.video();
    match name {
        "video-mode" => {
            let mode = VideoProcessingMode::from_code(need()?).ok_or_else(|| unknown("mode"))?;
            video.set_processing_mode(mode).await?;
        }
        "hdmi-output" => {
            let output = HdmiOutput::from_code(need()?).ok_or_else(|| unknown("output"))?;
            video.set_hdmi_output(output).await?;
        }
        "hdmi-audio" => {
            let decode = HdmiAudioDecode::from_code(need()?).ok_or_else(|| unknown("audio"))?;
            video.set_hdmi_audio(decode).await?;
        }
        "aspect" => {
            let aspect = AspectRatio::from_code(need()?).ok_or_else(|| unknown("aspect"))?;
            video.set_aspect_ratio(aspect).await?;
        }
        "vertical-stretch" => {
            let on = match need()? {
                "on" => true,
                "off" => false,
                _ => return Err(unknown("vertical stretch").into()),
            };
            video.set_vertical_stretch(on).await?;
        }
        "resolution" => {
            let resolution =
                VideoResolution::from_code(need()?).ok_or_else(|| unknown("resolution"))?;
            video.set_resolution(resolution).await?;
        }
        "upscaler" => {
            let upscaler = HdmiUpscaler::from_code(need()?).ok_or_else(|| unknown("upscaler"))?;
            video.set_hdmi_upscaler(upscaler).await?;
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
        "key" => zone.press(remote_key(need()?)?).await?,
        "denon-code" => {
            let code = need()?.parse().map_err(|_| "denon-code is a number")?;
            zone.press_denon_code(code).await?;
        }
        "marantz-code" => zone.press_marantz_code(need()?).await?,
        other => return Err(format!("unknown command {other}").into()),
    }
    Ok(())
}

/// The remote key with this name.
fn remote_key(name: &str) -> Result<RemoteKey, String> {
    Ok(match name {
        "next" => RemoteKey::NextTrack,
        "previous" => RemoteKey::PreviousTrack,
        "repeat-off" => RemoteKey::RepeatOff,
        "repeat-one" => RemoteKey::RepeatOne,
        "repeat-all" => RemoteKey::RepeatAll,
        "random-on" => RemoteKey::RandomOn,
        "random-off" => RemoteKey::RandomOff,
        "page-up" => RemoteKey::PageUp,
        "page-down" => RemoteKey::PageDown,
        "rewind" => RemoteKey::Rewind,
        "fast-forward" => RemoteKey::FastForward,
        "atmos" => RemoteKey::DolbyAtmosToggle,
        "cec-on" => RemoteKey::HdmiCecOn,
        "cec-off" => RemoteKey::HdmiCecOff,
        "volume-0db" => RemoteKey::MasterVolume0Db,
        "volume-minus-20db" => RemoteKey::MasterVolumeMinus20Db,
        "volume-minus-40db" => RemoteKey::MasterVolumeMinus40Db,
        other => match other.strip_prefix("digit-").and_then(|d| d.parse().ok()) {
            Some(digit) => RemoteKey::Digit(digit),
            None => return Err(format!("unknown key {other}")),
        },
    })
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
    let other = if typed(&ZONE_COMMANDS)
        || typed(&TONE_COMMANDS)
        || typed(&SOUND_COMMANDS)
        || typed(&VIDEO_COMMANDS)
    {
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
        None if typed(&VIDEO_COMMANDS) => {
            video_command(&avr, &name, argument.as_deref()).await?;
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
