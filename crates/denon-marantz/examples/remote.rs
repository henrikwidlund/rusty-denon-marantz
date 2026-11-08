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
//! - `--batch`: no command on the command line. The example keeps one connection and reads
//!   lines from the standard input, each `[--zone 2|3] COMMAND [ARGUMENT]`. After each line it prints
//!   `done N`. With `--groups ""` the connect asks for no state, so a test starts at once.
//! - In the batch mode, `refresh GROUP` asks for a group of values (for example `refresh core`).
//! - `--groups LIST`: the query groups that the connect fetches, in this order,
//!   for example `core,signal`. The groups are core, names, toggles, signal,
//!   device, sound and zones. The default is all of them.
//! - `--command-interval MS`, `--query-interval MS` and `--query-quiet MS`: the
//!   spacing of writes. A query also waits until the receiver sent no data for
//!   the quiet time.
//!
//! Commands: power-on, power-standby, device-power-on, device-power-standby,
//! tone-on, tone-off, bass DB, bass-up, bass-down, treble DB, treble-up,
//! treble-down (DB is a whole number from -6 to 6), dynamic-eq-on,
//! dynamic-eq-off, multi-eq CODE, ref-level CODE, dynamic-volume CODE, lfc-on,
//! lfc-off, containment 1-7, containment-up, containment-down, dirac CODE dirac-name NAME,
//! effect on or off, effect-level N, center-image N, center-gain N, step SETTING-up or SETTING-down
//! (audio-delay, delay-time, dialog-control, dimension, center-width, center-width-cew, lfe, bass-sync, auro-strength,
//! center-level, effect-level, center-image, center-gain, subwoofer1 to subwoofer4, for example lfe-up),
//! dimmer CODE (BRI, DIM, DAR, OFF), dimmer-next, eco CODE (ON, AUTO, OFF), remote-lock on or off,
//! picture-mode CODE (OFF, STD, MOV, VVD, STM, CTM, DAY, NGT), contrast DB, brightness DB, saturation DB,
//! contrast-up, contrast-down, brightness-up, brightness-down, saturation-up, saturation-down,
//! noise-reduction CODE (OFF, LOW, MID, HI), enhancer N, enhancer-up, enhancer-down,
//! digital-codec CODE (AUTO, PCM, DTS), favorite N, favorite-store N (N is 1 to 3),
//! bluetooth on or off, bluetooth-output SP or BT, illumination CODE (AUTO, BRI, DIM, DAR, OFF),
//! illumination-next, quick-select N, quick-select-store N (N is 1 to 5), panel-lock off, panel or panel-volume, channel-level-menu, show-status, restart-network,
//! sound-mode-next, sound-mode-previous, auto-standby CODE (OFF, 15M, 30M, 60M, 2H, 4H, 8H),
//! sleep off or MINUTES, zone-bass DB, zone-bass-up, zone-bass-down, zone-treble DB, zone-treble-up,
//! zone-treble-down, zone-left DB, zone-left-up, zone-left-down, zone-right DB, zone-right-up,
//! zone-right-down, zone-hpf on or off, zone-channel ST or MONO, zone-hdmi-audio THR or PCM,
//! volume-limit DB or off, power-on-volume muted, last or DB, mute-level CODE (MUT, 040, 060),
//! volume-mode variable or DB, volume-display REL or ABS (main zone only; the other zone
//! commands are for --zone 2 and --zone 3, volume-limit, power-on-volume and mute-level work for all),
//! room-size CODE, audio-restorer CODE, dialog-enhancer CODE, graphic-eq on or off, input-mode CODE (AUTO, HDMI, DIGITAL or ANALOG), front-speakers CODE (SPA, SPB, A+B), panorama on or off, speaker-preset 1 or 2, tactile on or off, tactile-up, tactile-down, tactile-lpf HZ, channel-volume CHANNEL DB (CHANNEL is FL, FR, C, SW, SL ..., Dirac must be off), channel-up CHANNEL, channel-down CHANNEL, channel-reset,
//! tuner-up, tuner-down, tuner-band CODE (AM, FM, DAB), tuner-mode CODE (AUTO or MANUAL),
//! tuner-preset-up, tuner-preset-down, tuner-preset N, tuner-store N (N is 1 to 56),
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

use denon_marantz::{
    Avr, Command, QueryGroup, RemoteKey, TelnetConfig, ToneControl, VideoControl, ZoneControl,
};
use denon_marantz_proto::{
    AspectRatio, AudioRestorer, AutoStandby, BluetoothOutputMode, Channel, ChannelLevel,
    DialogEnhancerLevel, DigitalCodec, DimmerMode, DiracFilter, DynamicVolume, EcoMode,
    FrontSpeakers, HdmiAudioDecode, HdmiOutput, HdmiUpscaler, Illumination, InputMode, MultiEq,
    MuteLevel, NoiseReduction, PanelLock, PictureMode, PowerOnVolume, ReferenceLevelOffset,
    RoomSize, Sleep, SoundSettingValue, SoundStep, SpeakerPreset, ToneLevel, Trigger, TunerBand,
    TunerMode, VideoProcessingMode, VideoResolution, Volume, VolumeDisplay, VolumeMode, Zone,
    ZoneChannelMode, ZoneHdmiAudio,
};
use tokio::sync::broadcast::error::RecvError;
use tokio::time::{Instant, sleep};
use tracing_subscriber::EnvFilter;

const USAGE: &str =
    "usage: remote <host> [options] <command> [argument]  (see the top of the example file)";

/// Commands that the zone control has methods for.
const ZONE_COMMANDS: [&str; 38] = [
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
    "zone-bass",
    "zone-bass-up",
    "zone-bass-down",
    "zone-treble",
    "zone-treble-up",
    "zone-treble-down",
    "zone-left",
    "zone-left-up",
    "zone-left-down",
    "zone-right",
    "zone-right-up",
    "zone-right-down",
    "zone-hpf",
    "zone-channel",
    "zone-hdmi-audio",
    "volume-limit",
    "power-on-volume",
    "mute-level",
    "volume-mode",
    "volume-display",
    "auto-standby",
    "sleep",
    "sound-mode-next",
    "sound-mode-previous",
    "quick-select",
    "quick-select-store",
];

/// Commands that the video control has methods for.
const VIDEO_COMMANDS: [&str; 21] = [
    "video-mode",
    "hdmi-output",
    "hdmi-audio",
    "aspect",
    "vertical-stretch",
    "resolution",
    "upscaler",
    "picture-mode",
    "contrast",
    "brightness",
    "saturation",
    "noise-reduction",
    "enhancer",
    "contrast-up",
    "contrast-down",
    "brightness-up",
    "brightness-down",
    "saturation-up",
    "saturation-down",
    "enhancer-up",
    "enhancer-down",
];

/// Commands that the device control has methods for.
const DEVICE_COMMANDS: [&str; 15] = [
    "dimmer",
    "dimmer-next",
    "eco",
    "remote-lock",
    "panel-lock",
    "bluetooth",
    "bluetooth-output",
    "illumination",
    "illumination-next",
    "digital-codec",
    "favorite",
    "favorite-store",
    "channel-level-menu",
    "show-status",
    "restart-network",
];

/// Commands that the tuner control has methods for.
const TUNER_COMMANDS: [&str; 8] = [
    "tuner-up",
    "tuner-down",
    "tuner-band",
    "tuner-mode",
    "tuner-preset-up",
    "tuner-preset-down",
    "tuner-preset",
    "tuner-store",
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
const SOUND_COMMANDS: [&str; 33] = [
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
    "tactile",
    "tactile-up",
    "tactile-down",
    "tactile-lpf",
    "front-speakers",
    "panorama",
    "speaker-preset",
    "effect",
    "effect-level",
    "center-image",
    "center-gain",
    "channel-volume",
    "channel-up",
    "channel-down",
    "channel-reset",
    "step",
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
        "tactile" => {
            let on = match need()? {
                "on" => true,
                "off" => false,
                _ => return Err(unknown("tactile transducer").into()),
            };
            avr.sound().set_tactile_transducer(on).await?;
        }
        "front-speakers" => {
            let speakers = FrontSpeakers::from_code(need()?).ok_or_else(|| unknown("speakers"))?;
            avr.sound().set_front_speakers(speakers).await?;
        }
        "panorama" => {
            let on = match need()? {
                "on" => true,
                "off" => false,
                _ => return Err(unknown("panorama").into()),
            };
            avr.sound().set_panorama(on).await?;
        }
        "speaker-preset" => {
            let preset = SpeakerPreset::from_code(need()?).ok_or_else(|| unknown("preset"))?;
            avr.sound().set_speaker_preset(preset).await?;
        }
        "tactile-up" => avr.sound().tactile_level_up().await?,
        "tactile-down" => avr.sound().tactile_level_down().await?,
        "tactile-lpf" => {
            let hz = need()?
                .parse()
                .map_err(|_| "tactile-lpf is a number of Hz")?;
            avr.sound().set_tactile_low_pass(hz).await?;
        }
        other => return sound_extra_command(avr, other, argument).await,
    }
    Ok(())
}

/// The sound commands that do not fit in `sound_command`: effect, center image and gain, steps.
async fn sound_extra_command(
    avr: &Avr,
    name: &str,
    argument: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let need = || argument.ok_or_else(|| format!("{name} needs an argument"));
    let unknown = |what: &str| format!("{what} is not known: {}", argument.unwrap_or_default());
    let small = |text: &str| -> Result<u8, String> {
        text.parse()
            .map_err(|_| "a whole number is needed".to_owned())
    };
    match name {
        "effect" => {
            let on = match need()? {
                "on" => true,
                "off" => false,
                _ => return Err(unknown("effect").into()),
            };
            avr.sound().set_effect(on).await?;
        }
        "effect-level" => avr.sound().set_effect_level(small(need()?)?).await?,
        "center-image" => avr.sound().set_center_image(small(need()?)?).await?,
        "center-gain" => avr.sound().set_center_gain(small(need()?)?).await?,
        "channel-volume" => {
            let (code, db) = need()?
                .split_once(' ')
                .ok_or("channel-volume is CHANNEL DB, for example FL -1.5")?;
            let channel = Channel::from_code(code).ok_or_else(|| unknown("channel"))?;
            let db: f32 = db.trim().parse().map_err(|_| "DB is a number in dB")?;
            let level = ChannelLevel::from_db(db).ok_or("DB is from -12 to 12 in steps of 0.5")?;
            avr.sound().set_channel_volume(channel, level).await?;
        }
        "channel-up" => {
            let channel = Channel::from_code(need()?).ok_or_else(|| unknown("channel"))?;
            avr.sound().channel_volume_up(channel).await?;
        }
        "channel-down" => {
            let channel = Channel::from_code(need()?).ok_or_else(|| unknown("channel"))?;
            avr.sound().channel_volume_down(channel).await?;
        }
        "channel-reset" => avr.sound().reset_channel_volumes().await?,
        "step" => {
            let (step, up) = parse_step(need()?)?;
            let value = if up {
                SoundSettingValue::StepUp(step)
            } else {
                SoundSettingValue::StepDown(step)
            };
            avr.send(Zone::Main, Command::SetSoundSetting(value))
                .await?;
        }
        other => return Err(format!("unknown command {other}").into()),
    }
    Ok(())
}

/// A step like `lfe-up` or `subwoofer2-down`: the setting, and up (`true`) or down.
fn parse_step(text: &str) -> Result<(SoundStep, bool), String> {
    let (setting, direction) = text
        .rsplit_once('-')
        .ok_or("a step is SETTING-up or SETTING-down")?;
    let up = match direction {
        "up" => true,
        "down" => false,
        _ => return Err("a step is SETTING-up or SETTING-down".into()),
    };
    let step = match setting {
        "audio-delay" => SoundStep::AudioDelay,
        "delay-time" => SoundStep::DelayTime,
        "dialog-control" => SoundStep::DialogControl,
        "dimension" => SoundStep::Dimension,
        "center-width" => SoundStep::CenterWidth,
        "center-width-cew" => SoundStep::CenterWidthCew,
        "lfe" => SoundStep::LfeLevel,
        "bass-sync" => SoundStep::BassSync,
        "auro-strength" => SoundStep::AuroMaticStrength,
        "center-level" => SoundStep::CenterLevel,
        "effect-level" => SoundStep::EffectLevel,
        "center-image" => SoundStep::CenterImage,
        "center-gain" => SoundStep::CenterGain,
        other => match other.strip_prefix("subwoofer").map(str::parse) {
            Some(Ok(number)) => SoundStep::SubwooferLevel(number),
            _ => return Err(format!("unknown setting {other}")),
        },
    };
    Ok((step, up))
}

/// Runs a command with the methods of the device control.
async fn device_command(
    avr: &Avr,
    name: &str,
    argument: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let need = || argument.ok_or_else(|| format!("{name} needs an argument"));
    let device = avr.device();
    match name {
        "dimmer" => {
            let mode = DimmerMode::from_telnet(need()?).ok_or("dimmer is BRI, DIM, DAR or OFF")?;
            device.set_dimmer(mode).await?;
        }
        "dimmer-next" => device.dimmer_next().await?,
        "eco" => {
            let mode = EcoMode::from_telnet(need()?).ok_or("eco is ON, AUTO or OFF")?;
            device.set_eco_mode(mode).await?;
        }
        "remote-lock" => match need()? {
            "on" => device.set_remote_lock(true).await?,
            "off" => device.set_remote_lock(false).await?,
            _ => return Err("use on or off".into()),
        },
        "panel-lock" => {
            let lock = match need()? {
                "off" => PanelLock::Off,
                "panel" => PanelLock::Panel,
                "panel-volume" => PanelLock::PanelAndVolume,
                _ => return Err("use off, panel or panel-volume".into()),
            };
            device.set_panel_lock(lock).await?;
        }
        "bluetooth" => match need()? {
            "on" => device.set_bluetooth_transmitter(true).await?,
            "off" => device.set_bluetooth_transmitter(false).await?,
            _ => return Err("use on or off".into()),
        },
        "bluetooth-output" => {
            let mode = BluetoothOutputMode::from_code(need()?).ok_or("output is SP or BT")?;
            device.set_bluetooth_output(mode).await?;
        }
        "illumination" => {
            let mode = Illumination::from_code(need()?)
                .ok_or("illumination is AUTO, BRI, DIM, DAR or OFF")?;
            device.set_illumination(mode).await?;
        }
        "illumination-next" => device.illumination_next().await?,
        "digital-codec" => {
            let mode =
                DigitalCodec::from_code(need()?).ok_or("digital codec is AUTO, PCM or DTS")?;
            device.set_digital_codec(mode).await?;
        }
        "favorite" => {
            let number = need()?.parse().map_err(|_| "favorite is 1 to 3")?;
            device.select_favorite(number).await?;
        }
        "favorite-store" => {
            let number = need()?.parse().map_err(|_| "favorite is 1 to 3")?;
            device.store_favorite(number).await?;
        }
        "channel-level-menu" => device.channel_level_menu().await?,
        "show-status" => device.show_status().await?,
        "restart-network" => device.restart_network().await?,
        other => return Err(format!("unknown command {other}").into()),
    }
    Ok(())
}

/// Runs a command with the methods of the tuner control.
async fn tuner_command(
    avr: &Avr,
    name: &str,
    argument: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let need = || argument.ok_or_else(|| format!("{name} needs an argument"));
    let unknown = |what: &str| format!("{what} is not known: {}", argument.unwrap_or_default());
    let number = || -> Result<u8, String> {
        need()?
            .parse()
            .map_err(|_| format!("{name} needs a preset number"))
    };
    let tuner = avr.tuner();
    match name {
        "tuner-up" => tuner.frequency_up().await?,
        "tuner-down" => tuner.frequency_down().await?,
        "tuner-band" => {
            let band = TunerBand::from_code(need()?).ok_or_else(|| unknown("band"))?;
            tuner.set_band(band).await?;
        }
        "tuner-mode" => {
            let mode = TunerMode::from_code(need()?).ok_or_else(|| unknown("mode"))?;
            tuner.set_mode(mode).await?;
        }
        "tuner-preset-up" => tuner.preset_up().await?,
        "tuner-preset-down" => tuner.preset_down().await?,
        "tuner-preset" => tuner.set_preset(number()?).await?,
        "tuner-store" => tuner.store_preset(number()?).await?,
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
        other => return picture_command(&video, other, argument).await,
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
    let digit = |text: &str| -> Result<u8, String> {
        text.parse()
            .map_err(|_| "a quick select is a number from 1 to 5".to_owned())
    };
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
        "quick-select" => zone.select_quick_select(digit(need()?)?).await?,
        "quick-select-store" => zone.store_quick_select(digit(need()?)?).await?,
        "sound-mode-next" => zone.sound_mode_next().await?,
        "sound-mode-previous" => zone.sound_mode_previous().await?,
        "key" => zone.press(remote_key(need()?)?).await?,
        "denon-code" => {
            let code = need()?.parse().map_err(|_| "denon-code is a number")?;
            zone.press_denon_code(code).await?;
        }
        "marantz-code" => zone.press_marantz_code(need()?).await?,
        other => return zone_setting_command(zone, other, argument).await,
    }
    Ok(())
}

/// Runs a command with the picture methods of the video control.
async fn picture_command(
    video: &VideoControl<'_>,
    name: &str,
    argument: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let need = || argument.ok_or_else(|| format!("{name} needs an argument"));
    let whole = || -> Result<i8, String> {
        need()?
            .parse()
            .map_err(|_| format!("{name} needs a whole number"))
    };
    match name {
        "picture-mode" => {
            let mode = PictureMode::from_telnet(need()?).ok_or("picture mode is not known")?;
            video.set_picture_mode(mode).await?;
        }
        "contrast" => video.set_contrast(whole()?).await?,
        "brightness" => video.set_brightness(whole()?).await?,
        "saturation" => video.set_saturation(whole()?).await?,
        "noise-reduction" => {
            let level = NoiseReduction::from_code(need()?)
                .ok_or("noise reduction is OFF, LOW, MID or HI")?;
            video.set_noise_reduction(level).await?;
        }
        "enhancer" => {
            let level = need()?.parse().map_err(|_| "enhancer is 0 to 12")?;
            video.set_enhancer(level).await?;
        }
        "contrast-up" => video.contrast_up().await?,
        "contrast-down" => video.contrast_down().await?,
        "brightness-up" => video.brightness_up().await?,
        "brightness-down" => video.brightness_down().await?,
        "saturation-up" => video.saturation_up().await?,
        "saturation-down" => video.saturation_down().await?,
        "enhancer-up" => video.enhancer_up().await?,
        "enhancer-down" => video.enhancer_down().await?,
        other => return Err(format!("unknown command {other}").into()),
    }
    Ok(())
}

/// Runs a command with the zone setting methods of the zone control.
async fn zone_setting_command(
    zone: ZoneControl<'_>,
    name: &str,
    argument: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let need = || argument.ok_or_else(|| format!("{name} needs an argument"));
    let tone = |text: &str| -> Result<ToneLevel, String> {
        let db: i8 = text.parse().map_err(|_| "a level is whole dB")?;
        ToneLevel::from_db(db).ok_or_else(|| "a level is from -10 to 10 dB".to_owned())
    };
    let whole = |text: &str| -> Result<i8, String> {
        text.parse().map_err(|_| "a level is whole dB".to_owned())
    };
    let on_off = |text: &str| -> Result<bool, String> {
        match text {
            "on" => Ok(true),
            "off" => Ok(false),
            _ => Err("use on or off".to_owned()),
        }
    };
    let volume = |text: &str| -> Result<Volume, String> {
        let db: f32 = text.parse().map_err(|_| "a volume is in dB")?;
        Volume::from_db_rounded(db).ok_or_else(|| "a volume is from -80 to 18 dB".to_owned())
    };
    match name {
        "zone-bass" => zone.set_bass(tone(need()?)?).await?,
        "zone-bass-up" => zone.bass_up().await?,
        "zone-bass-down" => zone.bass_down().await?,
        "zone-treble" => zone.set_treble(tone(need()?)?).await?,
        "zone-treble-up" => zone.treble_up().await?,
        "zone-treble-down" => zone.treble_down().await?,
        "zone-left" => zone.set_left_level(whole(need()?)?).await?,
        "zone-left-up" => zone.left_level_up().await?,
        "zone-left-down" => zone.left_level_down().await?,
        "zone-right" => zone.set_right_level(whole(need()?)?).await?,
        "zone-right-up" => zone.right_level_up().await?,
        "zone-right-down" => zone.right_level_down().await?,
        "zone-hpf" => zone.set_high_pass_filter(on_off(need()?)?).await?,
        "zone-channel" => {
            let mode = ZoneChannelMode::from_code(need()?).ok_or("channel is ST or MONO")?;
            zone.set_channel_mode(mode).await?;
        }
        "zone-hdmi-audio" => {
            let mode = ZoneHdmiAudio::from_code(need()?).ok_or("HDMI audio is THR or PCM")?;
            zone.set_hdmi_audio(mode).await?;
        }
        "volume-limit" => {
            let limit = match need()? {
                "off" => None,
                db => Some(volume(db)?),
            };
            zone.set_volume_limit(limit).await?;
        }
        "power-on-volume" => {
            let setting = match need()? {
                "muted" => PowerOnVolume::Muted,
                "last" => PowerOnVolume::Last,
                db => PowerOnVolume::Level(volume(db)?),
            };
            zone.set_power_on_volume(setting).await?;
        }
        "mute-level" => {
            let level = MuteLevel::from_code(need()?).ok_or("mute level is MUT, 040 or 060")?;
            zone.set_mute_level(level).await?;
        }
        "volume-mode" => {
            let mode = match need()? {
                "variable" => VolumeMode::Variable,
                db => VolumeMode::Fixed(volume(db)?),
            };
            zone.set_volume_mode(mode).await?;
        }
        "auto-standby" => {
            let standby = AutoStandby::from_telnet(need()?)
                .ok_or("auto standby is OFF, 15M, 30M, 60M, 2H, 4H or 8H")?;
            zone.set_auto_standby(standby).await?;
        }
        "sleep" => {
            let sleep = match need()? {
                "off" => Sleep::Off,
                minutes => minutes
                    .parse()
                    .ok()
                    .and_then(Sleep::minutes)
                    .ok_or("sleep is off or 1 to 120 minutes")?,
            };
            zone.set_sleep(sleep).await?;
        }
        "volume-display" => {
            let display = VolumeDisplay::from_code(need()?).ok_or("display is REL or ABS")?;
            zone.set_volume_display(display).await?;
        }
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

/// The query groups in a comma separated list, for example `core,signal`.
fn groups(names: &str) -> Result<Vec<QueryGroup>, String> {
    names
        .split(',')
        .filter(|name| !name.is_empty())
        .map(|name| {
            QueryGroup::ALL
                .iter()
                .copied()
                .find(|group| group.label().eq_ignore_ascii_case(name))
                .ok_or_else(|| format!("unknown group {name}"))
        })
        .collect()
}

/// The next argument as a number.
fn number(args: &mut impl Iterator<Item = String>, option: &str) -> Result<u64, String> {
    args.next()
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| format!("{option} needs a whole number"))
}

/// Runs one command. The zone is for the commands of a zone.
async fn run(
    avr: &Avr,
    zone: Zone,
    name: &str,
    argument: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let typed = |commands: &[&str]| commands.contains(&name);
    if name == "refresh" {
        // Asks for one group of values. Repeated values show again.
        let wanted = groups(argument.ok_or("refresh needs a group")?)?;
        for group in wanted {
            avr.telnet_refresh_group(group).await?;
        }
        Ok(())
    } else if typed(&SOUND_COMMANDS) {
        sound_command(avr, name, argument).await
    } else if typed(&DEVICE_COMMANDS) {
        device_command(avr, name, argument).await
    } else if typed(&TUNER_COMMANDS) {
        tuner_command(avr, name, argument).await
    } else if typed(&VIDEO_COMMANDS) {
        video_command(avr, name, argument).await
    } else if typed(&TONE_COMMANDS) {
        tone_command(avr.tone(), name, argument).await
    } else if typed(&ZONE_COMMANDS) {
        zone_command(avr.zone(zone)?, name, argument).await
    } else {
        avr.send(zone, command(name, argument)?).await?;
        Ok(())
    }
}

/// Refuses a name that is not a command, without a connection.
fn check_known(name: &str, argument: Option<&str>) -> Result<(), String> {
    let typed = |commands: &[&str]| commands.contains(&name);
    if name == "refresh"
        || typed(&ZONE_COMMANDS)
        || typed(&TONE_COMMANDS)
        || typed(&SOUND_COMMANDS)
        || typed(&VIDEO_COMMANDS)
        || typed(&TUNER_COMMANDS)
        || typed(&DEVICE_COMMANDS)
    {
        Ok(())
    } else {
        command(name, argument).map(|_| ())
    }
}

/// A line of the batch mode: `[--zone 2|3] COMMAND [ARGUMENT]`.
fn parse_line(line: &str) -> Result<(Zone, &str, Option<&str>), String> {
    let mut rest = line.trim();
    let mut zone = Zone::Main;
    if let Some(after) = rest.strip_prefix("--zone ") {
        let (number, tail) = after.trim_start().split_once(' ').unwrap_or((after, ""));
        zone = match number {
            "2" => Zone::Zone2,
            "3" => Zone::Zone3,
            _ => return Err("--zone is 2 or 3".into()),
        };
        rest = tail.trim_start();
    }
    let (name, argument) = match rest.split_once(' ') {
        Some((name, argument)) => (name, Some(argument.trim()).filter(|text| !text.is_empty())),
        None => (rest, None),
    };
    if name.is_empty() {
        return Err("empty line".into());
    }
    Ok((zone, name, argument))
}

/// Runs the lines of the standard input as commands, over one connection.
async fn run_batch(avr: &Avr, stamp: &(impl Fn() -> f64 + Sync)) {
    let (lines, mut received) = tokio::sync::mpsc::unbounded_channel();
    tokio::task::spawn_blocking(move || {
        for line in std::io::stdin().lines().map_while(Result::ok) {
            if lines.send(line).is_err() {
                break;
            }
        }
    });
    println!("[{:7.3}] ready", stamp());
    let mut done = 0_u32;
    while let Some(line) = received.recv().await {
        match parse_line(&line) {
            Ok((zone, name, argument)) => {
                println!("[{:7.3}] sending {name} to {zone:?}", stamp());
                match run(avr, zone, name, argument).await {
                    Ok(()) => println!("[{:7.3}] written", stamp()),
                    Err(error) => println!("[{:7.3}] failed: {error}", stamp()),
                }
            }
            Err(error) => println!("[{:7.3}] failed: {error}", stamp()),
        }
        done += 1;
        println!("[{:7.3}] done {done}", stamp());
    }
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
    let mut batch = false;
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
            "--groups" => {
                let names = args.next().ok_or("--groups needs a list")?;
                config.query_groups = Box::leak(groups(&names)?.into_boxed_slice());
            }
            "--batch" => {
                batch = true;
                break String::new();
            }
            _ => break word,
        }
    };
    let argument = args.next();
    if !batch {
        // A command that is not known is refused before the connect.
        check_known(&name, argument.as_deref())?;
    }

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

    if batch {
        run_batch(&avr, &stamp).await;
    } else {
        sleep(wait).await;
        println!("[{:7.3}] sending {name} to {zone:?}", stamp());
        run(&avr, zone, &name, argument.as_deref()).await?;
        println!("[{:7.3}] written", stamp());
        sleep(listen).await;
    }

    printer.abort();
    avr.telnet_disconnect().await;
    Ok(())
}
