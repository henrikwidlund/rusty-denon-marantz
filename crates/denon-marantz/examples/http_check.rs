//! Checks commands over HTTP against a receiver: which commands work, how fast
//! they can follow each other, and what the reads after them cost. Every command
//! goes over HTTP. The phases `queries`, `effects`, `power`, `main-power`,
//! `switch` and `switch-telnet` open telnet to see what the receiver did. The
//! phases with `http-` in the name, `rate` and `load` do not open it.
//!
//! ```sh
//! cargo run --release -p denon-marantz --example http_check -- 192.168.1.20 queries
//! cargo run --release -p denon-marantz --example http_check -- 192.168.1.20 effects
//! cargo run --release -p denon-marantz --example http_check -- 192.168.1.20 http-effects
//! cargo run --release -p denon-marantz --example http_check -- 192.168.1.20 rate 50 40
//! cargo run --release -p denon-marantz --example http_check -- 192.168.1.20 load 10 50 --no-follow-up
//! ```
//!
//! The phases that only read:
//!
//! - `queries`: asks `MNZST?` on telnet, on its own connection. Sends nothing
//!   else.
//!
//! The phases that change the receiver for a moment and put it back:
//!
//! - `effects`: mute, the volume by one step, the source, the sound mode, bass,
//!   Dynamic EQ and all zone stereo, one after the other. Telnet shows what
//!   happened.
//! - `http-effects`: mute, volume, source and sound mode, without telnet. The
//!   state changes by the read after the command, so the time is the time of
//!   that read.
//! - `rate INTERVAL_MS COUNT`: `COUNT` commands, one each `INTERVAL_MS`, as
//!   volume up and down in turns, so the volume is never more than one step off.
//!   `INTERVAL_MS` is above 0, and `COUNT` up to 100000.
//! - `load SECONDS INTERVAL_MS`: the same for `SECONDS` (up to 3600), while the
//!   state is read once every 500 ms to see how slow the receiver is.
//!   `--no-follow-up` turns off the reads after the commands.
//! - `switch` and `http-switch`: sends a second source command a short time
//!   after the first, and says which source the receiver ends on. With
//!   `switch-telnet` the commands go over telnet, as a control.
//!
//! The phases that switch power. Use them only when you can switch the receiver
//! on again by hand:
//!
//! - `power` and `http-power`: zone 2 and zone 3 on or off, and back.
//! - `main-power` and `http-main-power`: the main zone, and then the whole
//!   receiver, in standby and on again. The power on sets the volume to the
//!   power-on level of the receiver, and `PWON` also switches on zone 2 and
//!   zone 3.
//!
//! Each result is a line that starts with `RESULT`. A phase that changes
//! something records the command that puts it back before it sends the change.
//! If the example stops with an error or with Ctrl-C, it sends the recorded
//! commands. The phases that change the volume, the mute or the source check
//! them at the end and put them back. The output can have personal data, for
//! example the name of the receiver.

// A tool for hand tests: long functions and rounded numbers are fine here.
#![allow(
    clippy::too_many_lines,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::future_not_send
)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use denon_marantz::proto::MultiEq;
use denon_marantz::proto::Power;
use denon_marantz::proto::source::display_to_select_code;
use denon_marantz::{
    Avr, Command, FollowUpRead, QueryGroup, StartOptions, State, TelnetStart, ToneLevel, Volume,
    Zone,
};
use tokio::time::{MissedTickBehavior, interval, sleep, timeout};
use tracing_subscriber::EnvFilter;

const USAGE: &str = "usage: http_check <host> queries | effects | http-effects | power | \
                     http-power | main-power | http-main-power | switch | http-switch | \
                     switch-telnet | rate INTERVAL_MS COUNT | \
                     load SECONDS INTERVAL_MS [--no-follow-up]";

const PHASES: [&str; 12] = [
    "queries",
    "effects",
    "http-effects",
    "power",
    "http-power",
    "main-power",
    "http-main-power",
    "switch",
    "http-switch",
    "switch-telnet",
    "rate",
    "load",
];

/// The commands that put back what a phase changed, as telnet lines. A phase
/// adds one before it changes something, and takes it away when the value is
/// back. Ctrl-C or an error leaves the rest, and they are sent at the end.
static UNDO: Mutex<Vec<(Zone, String)>> = Mutex::new(Vec::new());

fn remember(avr: &Avr, zone: Zone, back: &Command<'_>) {
    let mut line = String::new();
    if back.write_line(avr.info().brand, zone, &mut line).is_ok() {
        UNDO.lock().unwrap().push((zone, line));
    }
}

fn forget() {
    UNDO.lock().unwrap().pop();
}

async fn undo_pending(avr: &Avr) {
    let pending = std::mem::take(&mut *UNDO.lock().unwrap());
    for (zone, line) in pending.into_iter().rev() {
        let text = line.trim_end();
        println!("RESULT undo {zone:?} {text:?}");
        let _ = avr.http_send(zone, Command::Raw(text)).await;
        sleep(Duration::from_secs(1)).await;
    }
}

/// What the example puts back.
struct Original {
    volume: Volume,
    muted: bool,
    source: Option<String>,
    sound_mode: Option<String>,
    bass: Option<i8>,
    dynamic_eq: Option<bool>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let mut args = std::env::args().skip(1);
    let host = args.next().ok_or(USAGE)?;
    let phase = args.next().ok_or(USAGE)?;
    if !PHASES.contains(&phase.as_str()) {
        return Err(USAGE.into());
    }
    let rest: Vec<String> = args.collect();
    let follow_up = (!rest.iter().any(|a| a == "--no-follow-up")).then(FollowUpRead::default);
    let number = |index: usize| -> Result<u64, Box<dyn std::error::Error>> {
        Ok(rest.get(index).ok_or(USAGE)?.parse()?)
    };
    // The numbers are checked before anything is sent.
    let numbers = match phase.as_str() {
        "rate" => {
            let (every, count) = (number(0)?, number(1)?);
            (every > 0 && (1..=100_000).contains(&count))
                .then_some((every, count))
                .ok_or("rate: INTERVAL_MS is above 0 and COUNT is from 1 to 100000")?
        }
        "load" => {
            let (seconds, every) = (number(0)?, number(1)?);
            ((1..=3600).contains(&seconds) && every > 0)
                .then_some((seconds, every))
                .ok_or("load: SECONDS is from 1 to 3600 and INTERVAL_MS is above 0")?
        }
        _ => (0, 0),
    };
    if phase == "queries" {
        // On its own connections, with no start.
        return queries(&host).await;
    }

    let avr = Arc::new(
        Avr::builder(host.clone())
            .follow_up_read(follow_up)
            .connect()
            .await?,
    );
    let telnet = matches!(
        phase.as_str(),
        "effects" | "power" | "main-power" | "switch" | "switch-telnet"
    );
    let started = avr
        .start_with(StartOptions::new().signal_pages(false).telnet(telnet))
        .await?;
    let report = started.finished().await;
    println!("RESULT telnet={:?}", report.telnet);
    if telnet && !matches!(report.telnet, TelnetStart::Open) {
        return Err("telnet is needed to see what the commands did".into());
    }
    let original = original(&avr.snapshot()).ok_or("the receiver did not report its values")?;
    println!(
        "RESULT original volume_db={} muted={} source={:?} sound_mode={:?}",
        original.volume.db(),
        original.muted,
        original.source,
        original.sound_mode
    );

    let run = async {
        match phase.as_str() {
            "effects" => effects(&avr, &original, true).await,
            "http-effects" => effects(&avr, &original, false).await,
            "power" | "http-power" => power(&avr).await,
            "main-power" | "http-main-power" => main_power(&avr, telnet).await,
            "switch" | "http-switch" => switch(&avr, &original, false).await,
            "switch-telnet" => switch(&avr, &original, true).await,
            "rate" => rate(&avr, Duration::from_millis(numbers.0), numbers.1, None).await,
            "load" => {
                let seconds = Duration::from_secs(numbers.0);
                let every = Duration::from_millis(numbers.1);
                load(&avr, seconds, every).await
            }
            _ => Err(USAGE.into()),
        }
    };
    let result = tokio::select! {
        result = run => result,
        _ = tokio::signal::ctrl_c() => Err("stopped".into()),
    };
    undo_pending(&avr).await;
    if matches!(phase.as_str(), "rate" | "load") {
        // The receiver needs a moment to do the last command.
        sleep(Duration::from_millis(700)).await;
    }
    if matches!(phase.as_str(), "rate" | "load") && avr.update().await.is_ok() {
        // A command that the receiver dropped leaves the volume off.
        let now = avr.snapshot().zone(Zone::Main).volume.map(Volume::db);
        println!(
            "RESULT drift volume_db_now={now:?} original={} same={}",
            original.volume.db(),
            now == Some(original.volume.db())
        );
    }
    if phase.contains("switch") {
        put_back_source(&avr, &original).await;
    }
    if !matches!(
        phase.as_str(),
        "queries" | "power" | "http-power" | "switch" | "switch-telnet" | "http-switch"
    ) {
        put_back(
            &avr,
            &original,
            phase == "effects",
            phase.ends_with("effects"),
        )
        .await;
    }
    result
}

fn original(state: &State) -> Option<Original> {
    let main = state.zone(Zone::Main);
    Some(Original {
        volume: main.volume?,
        muted: main.muted?,
        source: main.source.clone(),
        sound_mode: main.sound_mode.clone(),
        bass: main.bass.map(ToneLevel::db),
        dynamic_eq: state.device().dynamic_eq,
    })
}

/// Waits until `condition` is true for the state. Returns the time it took.
async fn wait_for(avr: &Avr, condition: impl Fn(&State) -> bool) -> Option<Duration> {
    wait_for_within(avr, Duration::from_secs(3), condition).await
}

async fn wait_for_within(
    avr: &Avr,
    limit: Duration,
    condition: impl Fn(&State) -> bool,
) -> Option<Duration> {
    let begin = Instant::now();
    while begin.elapsed() < limit {
        if condition(&avr.snapshot()) {
            return Some(begin.elapsed());
        }
        sleep(Duration::from_millis(5)).await;
    }
    None
}

/// Asks each query on a new telnet connection, so that an answer that repeats
/// an older value is still seen as a change of the empty state.
async fn queries(host: &str) -> Result<(), Box<dyn std::error::Error>> {
    for query in ["MNZST?"] {
        let avr = Avr::builder(host).connect().await?;
        avr.telnet_connect().await?;
        let begin = Instant::now();
        avr.telnet_send(Zone::Main, Command::Raw(query)).await?;
        let answer = wait_for(&avr, |s| s.device().all_zone_stereo.is_some()).await;
        let value = avr.snapshot().device().all_zone_stereo;
        println!(
            "RESULT query={query:?} answered={} value={value:?} after_ms={:.0}",
            answer.is_some(),
            begin.elapsed().as_secs_f64() * 1000.0
        );
        avr.telnet_disconnect().await;
    }
    Ok(())
}

/// Sends `command` over HTTP, and says if `check` was true afterwards.
async fn effect(
    avr: &Avr,
    name: &str,
    command: Command<'_>,
    back: Command<'_>,
    check: impl Fn(&State) -> bool,
    check_back: impl Fn(&State) -> bool,
) {
    effect_in(
        avr,
        Zone::Main,
        Duration::from_secs(3),
        name,
        command,
        back,
        check,
        check_back,
    )
    .await;
}

/// Like `effect`, for a zone, and the state can take `limit` to change.
#[allow(clippy::too_many_arguments)] // A test tool.
async fn effect_in(
    avr: &Avr,
    zone: Zone,
    limit: Duration,
    name: &str,
    command: Command<'_>,
    back: Command<'_>,
    check: impl Fn(&State) -> bool,
    check_back: impl Fn(&State) -> bool,
) {
    remember(avr, zone, &back);
    let sent = Instant::now();
    let answer = avr.http_send(zone, command).await;
    let http_ms = sent.elapsed().as_secs_f64() * 1000.0;
    let seen = match &answer {
        Ok(()) => wait_for_within(avr, limit, check).await,
        Err(_) => None,
    };
    println!(
        "RESULT effect {name}: http={} http_ms={http_ms:.0} seen_ms={}",
        answer
            .as_ref()
            .map_or_else(|e| format!("ERROR {e}"), |()| "OK".into()),
        seen.map_or_else(
            || "NO EFFECT".into(),
            |d| format!("{:.0}", d.as_secs_f64() * 1000.0)
        ),
    );
    let mut put_back = false;
    if answer.is_ok() {
        let _ = avr.http_send(zone, back).await;
        let back_seen = wait_for_within(avr, limit, check_back).await;
        put_back = back_seen.is_some();
        println!("RESULT effect {name}: put back={put_back}");
    }
    // The undo command stays when the value is not known to be back. It is
    // sent at the end. A command that sets the value again does no harm.
    if put_back {
        forget();
    }
    sleep(Duration::from_millis(300)).await;
}

async fn effects(
    avr: &Avr,
    original: &Original,
    telnet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let volume = |s: &State| s.zone(Zone::Main).volume.map(Volume::db);
    let up = volume(&avr.snapshot()).unwrap_or_default() + 0.5;
    let at = original.volume.db();
    effect(
        avr,
        "mute",
        Command::Mute(!original.muted),
        Command::Mute(original.muted),
        |s| s.zone(Zone::Main).muted == Some(!original.muted),
        |s| s.zone(Zone::Main).muted == Some(original.muted),
    )
    .await;
    effect(
        avr,
        "volume up",
        Command::VolumeUp,
        Command::VolumeDown,
        |s| volume(s) == Some(up),
        |s| volume(s) == Some(at),
    )
    .await;
    if let Some(volume_up) = Volume::from_db_rounded(at + 0.5) {
        effect(
            avr,
            "set volume",
            Command::SetVolume(volume_up),
            Command::SetVolume(original.volume),
            |s| volume(s) == Some(at + 0.5),
            |s| volume(s) == Some(at),
        )
        .await;
    }
    let sources = avr.snapshot();
    // The state can hold the display name or the telnet name of the source.
    let current = original
        .source
        .as_deref()
        .map(|name| display_to_select_code(name).unwrap_or(name));
    let other = sources
        .device()
        .sources
        .as_slice()
        .iter()
        .find(|info| info.in_use == Some(true) && Some(info.code.as_str()) != current)
        .map(|info| info.code.clone());
    if let (Some(other), Some(current)) = (other, current) {
        let home = original.source.clone();
        // The receiver can ignore a source command about 3 s after another one,
        // so the source is put back after 6 s at the latest.
        effect_in(
            avr,
            Zone::Main,
            Duration::from_secs(6),
            &format!("source {other}"),
            Command::SelectSource(&other),
            Command::SelectSource(current),
            |s| s.zone(Zone::Main).source != home,
            |s| s.zone(Zone::Main).source == home,
        )
        .await;
    } else {
        println!("RESULT effect source: skipped, no second source in use");
    }
    if let Some(mode) = original.sound_mode.as_deref() {
        // Next and previous put the mode back exactly. The name that the
        // receiver reports is not always the one that selects it.
        let (other, back) = if telnet {
            (
                if mode.contains("STEREO") {
                    "MOVIE"
                } else {
                    "STEREO"
                },
                mode,
            )
        } else {
            ("RIGHT", "LEFT")
        };
        effect(
            avr,
            &format!("sound mode {other}"),
            Command::SelectSoundMode(other),
            Command::SelectSoundMode(back),
            |s| s.zone(Zone::Main).sound_mode.as_deref() != Some(mode),
            |s| s.zone(Zone::Main).sound_mode.as_deref() == Some(mode),
        )
        .await;
    }
    if !telnet {
        println!("RESULT effect bass, dynamic eq, all zone stereo: skipped, they need telnet");
        return Ok(());
    }
    if let Some(bass) = original
        .bass
        .filter(|_| avr.snapshot().device().tone_control == Some(true))
    {
        let bass_db = |s: &State| s.zone(Zone::Main).bass.map(ToneLevel::db);
        effect(
            avr,
            "bass up",
            Command::BassUp,
            Command::BassDown,
            |s| bass_db(s) == Some(bass + 1),
            |s| bass_db(s) == Some(bass),
        )
        .await;
    } else {
        println!("RESULT effect bass: skipped, tone control is off");
    }
    // The receiver ignores Dynamic EQ while MultEQ is off, on telnet too.
    avr.telnet_refresh_group(QueryGroup::Sound).await.ok();
    wait_for(avr, |s| s.device().multi_eq.is_some()).await;
    if avr.snapshot().device().multi_eq == Some(MultiEq::Off) {
        println!("RESULT effect dynamic eq: skipped, MultEQ is off and the receiver ignores it");
    } else if let Some(on) = original.dynamic_eq {
        effect(
            avr,
            "dynamic eq",
            Command::DynamicEq(!on),
            Command::DynamicEq(on),
            |s| s.device().dynamic_eq == Some(!on),
            |s| s.device().dynamic_eq == Some(on),
        )
        .await;
    }
    avr.telnet_send(Zone::Main, Command::Raw("MNZST?"))
        .await
        .ok();
    wait_for(avr, |s| s.device().all_zone_stereo.is_some()).await;
    let zst = avr.snapshot().device().all_zone_stereo.unwrap_or(false);
    effect(
        avr,
        "all zone stereo",
        Command::AllZoneStereo(!zst),
        Command::AllZoneStereo(zst),
        |s| s.device().all_zone_stereo == Some(!zst),
        |s| s.device().all_zone_stereo == Some(zst),
    )
    .await;
    Ok(())
}

fn percentile(sorted: &[f64], part: f64) -> f64 {
    sorted
        .get(((sorted.len() as f64 - 1.0) * part).round() as usize)
        .copied()
        .unwrap_or(0.0)
}

/// Sends `count` commands, one each `every`, volume up and down in turns.
/// Returns the time of each send in milliseconds, and the number of errors.
async fn rate(
    avr: &Avr,
    every: Duration,
    count: u64,
    deadline: Option<Instant>,
) -> Result<(), Box<dyn std::error::Error>> {
    let begin = Instant::now();
    let mut ticker = interval(every);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let (mut times, mut errors, mut net) = (Vec::new(), 0_u32, 0_i32);
    for index in 0..count {
        ticker.tick().await;
        if deadline.is_some_and(|end| Instant::now() >= end) {
            break;
        }
        let command = if index % 2 == 0 {
            Command::VolumeUp
        } else {
            Command::VolumeDown
        };
        let sent = Instant::now();
        let step = if index % 2 == 0 { 1 } else { -1 };
        if avr.http_send(Zone::Main, command).await.is_err() {
            errors += 1;
        } else {
            net += step;
        }
        times.push(sent.elapsed().as_secs_f64() * 1000.0);
    }
    let elapsed = begin.elapsed().as_secs_f64();
    // End where the volume began, so the next test starts there.
    for _ in 0..net.abs() {
        let back = if net > 0 {
            Command::VolumeDown
        } else {
            Command::VolumeUp
        };
        let _ = avr.http_send(Zone::Main, back).await;
    }
    times.sort_by(f64::total_cmp);
    println!(
        "RESULT rate every_ms={} sent={} errors={errors} per_second={:.1} send_ms p50={:.0} p95={:.0} max={:.0}",
        every.as_millis(),
        times.len(),
        times.len() as f64 / elapsed,
        percentile(&times, 0.5),
        percentile(&times, 0.95),
        percentile(&times, 1.0),
    );
    Ok(())
}

/// Sends commands for `seconds`, and reads the state every 500 ms meanwhile.
async fn load(
    avr: &Avr,
    seconds: Duration,
    every: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    let end = Instant::now() + seconds;
    let count = seconds.as_millis() as u64 / every.as_millis().max(1) as u64 + 1;
    let probe = async {
        let mut times = Vec::new();
        while Instant::now() < end {
            let begin = Instant::now();
            if avr.update().await.is_ok() {
                times.push(begin.elapsed().as_secs_f64() * 1000.0);
            }
            sleep(Duration::from_millis(500)).await;
        }
        times.sort_by(f64::total_cmp);
        times
    };
    let (sent, times) = tokio::join!(rate(avr, every, count, Some(end)), probe);
    sent?;
    println!(
        "RESULT load reads={} update_ms p50={:.0} p95={:.0} max={:.0}",
        times.len(),
        percentile(&times, 0.5),
        percentile(&times, 0.95),
        percentile(&times, 1.0),
    );
    // Let the last read after the commands happen.
    sleep(Duration::from_secs(3)).await;
    Ok(())
}

/// Puts back the volume and what the other phases change.
async fn put_back(avr: &Avr, original: &Original, sound_mode: bool, source: bool) {
    let _ = timeout(Duration::from_secs(3), async {
        let _ = avr
            .http_send(Zone::Main, Command::SetVolume(original.volume))
            .await;
        let _ = avr
            .http_send(Zone::Main, Command::Mute(original.muted))
            .await;
        if let Some(mode) = original.sound_mode.as_deref().filter(|_| sound_mode) {
            let _ = avr
                .http_send(Zone::Main, Command::SelectSoundMode(mode))
                .await;
        }
    })
    .await;
    let back = wait_for(avr, |s| {
        s.zone(Zone::Main).volume == Some(original.volume)
            && s.zone(Zone::Main).muted == Some(original.muted)
    })
    .await;
    println!("RESULT put_back volume_and_mute={}", back.is_some());
    if source {
        put_back_source(avr, original).await;
    }
}

/// Reads the receiver, and puts the source back if it differs. The state can
/// be old right after a command, so this reads again first.
async fn put_back_source(avr: &Avr, original: &Original) {
    let Some(home) = original.source.as_deref() else {
        return;
    };
    let code = display_to_select_code(home).unwrap_or(home);
    for _ in 0..2 {
        sleep(Duration::from_secs(3)).await;
        if avr.update().await.is_ok()
            && avr.snapshot().zone(Zone::Main).source.as_deref() == Some(home)
        {
            break;
        }
        let _ = avr.http_send(Zone::Main, Command::SelectSource(code)).await;
    }
    sleep(Duration::from_secs(3)).await;
    let _ = avr.update().await;
    let state = avr.snapshot();
    let main = state.zone(Zone::Main);
    println!(
        "RESULT final source={:?} (was {:?}) sound_mode={:?} (was {:?}) same={}",
        main.source,
        original.source,
        main.sound_mode,
        original.sound_mode,
        main.source == original.source && main.sound_mode == original.sound_mode
    );
}

/// Turns zone 2 and zone 3 on or off over HTTP, and puts them back.
async fn power(avr: &Avr) -> Result<(), Box<dyn std::error::Error>> {
    for zone in [Zone::Zone2, Zone::Zone3] {
        if !avr.info().zones.contains(&zone) {
            println!("RESULT effect power {zone:?}: skipped, the receiver has no such zone");
            continue;
        }
        let Some(was) = avr.snapshot().zone(zone).power else {
            println!("RESULT effect power {zone:?}: skipped, the power is not known");
            continue;
        };
        let on = was == Power::On;
        let (forward, back) = if on {
            (Command::PowerStandby, Command::PowerOn)
        } else {
            (Command::PowerOn, Command::PowerStandby)
        };
        let wanted = if on { Power::Standby } else { Power::On };
        effect_in(
            avr,
            zone,
            Duration::from_secs(10),
            &format!("power {zone:?} (was {was:?})"),
            forward,
            back,
            |s| s.zone(zone).power == Some(wanted),
            |s| s.zone(zone).power == Some(was),
        )
        .await;
    }
    Ok(())
}

/// Sends two source commands, the second one `delay` after the first, while the
/// receiver still switches. Says which source the receiver ends on.
async fn switch(
    avr: &Avr,
    original: &Original,
    telnet: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let home = original
        .source
        .as_deref()
        .ok_or("the source is not known")?;
    let home = display_to_select_code(home).unwrap_or(home);
    let other = avr
        .snapshot()
        .device()
        .sources
        .as_slice()
        .iter()
        .find(|info| info.in_use == Some(true) && info.code != home)
        .map(|info| info.code.clone())
        .ok_or("no second source in use")?;
    let way = if telnet { "telnet" } else { "http" };
    for delay in [100_u64, 300, 600, 1000, 1500, 3000, 5000] {
        for command in [Command::SelectSource(&other), Command::SelectSource(home)] {
            let sent = if telnet {
                avr.telnet_send(Zone::Main, command).await
            } else {
                avr.http_send(Zone::Main, command).await
            };
            sent?;
            if command == Command::SelectSource(&other) {
                sleep(Duration::from_millis(delay)).await;
            }
        }
        // Telnet reports the end. Without telnet, read it later.
        sleep(Duration::from_secs(4)).await;
        let _ = avr.update().await;
        let now = avr.snapshot().zone(Zone::Main).source.clone();
        let lost = now.as_deref() != original.source.as_deref();
        println!(
            "RESULT switch via={way} delay_ms={delay} first={other} second={home} final={now:?} second_command_lost={lost}"
        );
        if lost {
            put_back_source(avr, original).await;
        }
    }
    Ok(())
}

/// Puts the main zone, and then the whole receiver, in standby and on again,
/// over HTTP. With telnet open, a receiver that does not wake is switched on
/// over telnet.
async fn main_power(avr: &Avr, telnet: bool) -> Result<(), Box<dyn std::error::Error>> {
    let pause = Duration::from_secs(5);
    let on = |s: &State| s.zone(Zone::Main).power == Some(Power::On);
    let off = |s: &State| matches!(s.zone(Zone::Main).power, Some(Power::Off | Power::Standby));
    if !on(&avr.snapshot()) {
        return Err("the main zone is not on, nothing to test".into());
    }
    for (name, standby, wake) in [
        ("zone", Command::PowerStandby, Command::PowerOn),
        (
            "device",
            Command::DevicePowerStandby,
            Command::DevicePowerOn,
        ),
    ] {
        let begin = Instant::now();
        remember(avr, Zone::Main, &wake);
        let sent = avr.http_send(Zone::Main, standby).await;
        let seen = wait_for_within(avr, Duration::from_secs(15), off).await;
        println!(
            "RESULT main_power {name} standby: http={} seen_ms={}",
            sent.as_ref()
                .map_or_else(|e| format!("ERROR {e}"), |()| "OK".into()),
            seen.map_or_else(
                || "NO EFFECT".into(),
                |d| format!("{:.0}", d.as_secs_f64() * 1000.0)
            ),
        );
        sleep(pause).await;
        let woken = avr.http_send(Zone::Main, wake).await;
        let seen_on = wait_for_within(avr, Duration::from_secs(30), on).await;
        println!(
            "RESULT main_power {name} on: http={} seen_ms={} (total {:.0} s)",
            woken
                .as_ref()
                .map_or_else(|e| format!("ERROR {e}"), |()| "OK".into()),
            seen_on.map_or_else(
                || "NO EFFECT".into(),
                |d| format!("{:.0}", d.as_secs_f64() * 1000.0)
            ),
            begin.elapsed().as_secs_f64()
        );
        if seen_on.is_none() {
            if telnet {
                let _ = avr.telnet_send(Zone::Main, Command::DevicePowerOn).await;
                let rescued = wait_for_within(avr, Duration::from_secs(30), on).await;
                println!(
                    "RESULT main_power rescue over telnet: on={}",
                    rescued.is_some()
                );
            }
            return Err("the receiver did not come on over HTTP".into());
        }
        forget();
        sleep(pause).await;
    }
    Ok(())
}
