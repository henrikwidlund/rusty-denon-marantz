//! Starts a receiver with `Avr::start` and prints when each step is done, what
//! the start read and the main values of the state.
//!
//! ```sh
//! cargo run --release -p denon-marantz --example start -- 192.168.1.20
//! cargo run --release -p denon-marantz --example start -- 192.168.1.20 --no-signal-pages --listen 5
//! ```
//!
//! Options after the address:
//!
//! - `--no-signal-pages`: do not read the signal pages of the web interface.
//! - `--no-telnet`: do not open telnet.
//! - `--listen SECONDS`: show the changes of the state for this long after
//!   the start is finished, default 3. Use a whole number.
//!
//! The example sends read-only requests. The output can have personal data,
//! for example the name of the receiver.

use std::sync::Arc;
use std::time::{Duration, Instant};

use denon_marantz::{Avr, StartOptions, TelnetStart, Zone};
use tokio::time::timeout;

const USAGE: &str = "usage: start <host> [--no-signal-pages] [--no-telnet] [--listen SECONDS]";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let host = args.next().ok_or(USAGE)?;
    let mut options = StartOptions::new();
    let mut listen = 3;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--no-signal-pages" => options = options.signal_pages(false),
            "--no-telnet" => options = options.telnet(false),
            "--listen" => listen = args.next().ok_or(USAGE)?.parse()?,
            _ => return Err(USAGE.into()),
        }
    }

    let begin = Instant::now();
    let secs = || begin.elapsed().as_secs_f64();
    let avr = Arc::new(Avr::builder(host).connect().await?);
    println!("RESULT connected_s={:.2}", secs());
    let started = avr.start_with(options).await?;
    println!("RESULT started_s={:.2}", secs());
    // When the signal is in the state: the first value, and all of them.
    let mut first = None;
    let ready = loop {
        let snapshot = avr.snapshot();
        let signal = &snapshot.device().signal;
        first = first.or_else(|| signal.audio_signal.as_ref().map(|_| secs()));
        if signal.audio_signal.is_some()
            && signal.channels.output.is_some()
            && signal.video.input.is_some()
        {
            break Some(secs());
        }
        if secs() > 8.0 {
            break None;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    };
    println!("RESULT signal_first_s={first:?} signal_ready_s={ready:?}");
    let mut changes = avr.changes();
    let report = started.finished().await;
    println!("RESULT finished_s={:.2}", secs());

    let telnet = match &report.telnet {
        TelnetStart::Open => "Open".to_owned(),
        TelnetStart::AlreadyRunning => "AlreadyRunning".to_owned(),
        TelnetStart::Disabled => "Disabled".to_owned(),
        TelnetStart::Failed(error) => format!("Failed ({error})"),
        _ => "Unknown".to_owned(),
    };
    println!("RESULT telnet={telnet}");
    match &report.web_interface {
        Ok(pages) => {
            let read = pages
                .availability
                .pages
                .iter()
                .filter(|(_, s)| s.is_read())
                .count();
            println!(
                "RESULT availability_read={read}/{}",
                pages.availability.pages.len()
            );
            println!("RESULT video_page={:?}", pages.video_signal);
            println!("RESULT channels_page={:?}", pages.audio_channels);
        }
        Err(error) => println!("RESULT web_interface=Error ({error})"),
    }

    // The values from telnet arrive as events after the report.
    let mut count = 0_u32;
    let end = Instant::now() + Duration::from_secs(listen);
    while let Ok(Ok(change)) = timeout(
        end.saturating_duration_since(Instant::now()),
        changes.recv(),
    )
    .await
    {
        count += 1;
        println!("[{:6.2}s] change {change:?}", secs());
    }
    println!("RESULT changes_while_listening={count}");
    print_state(&avr);
    Ok(())
}

fn print_state(avr: &Avr) {
    let state = avr.snapshot();
    let main = state.zone(Zone::Main);
    println!(
        "STATE power={:?} source={:?} volume={:?} muted={:?}",
        main.power, main.source, main.volume, main.muted
    );
    println!(
        "STATE sound_mode={:?} quick_select={:?}",
        main.sound_mode, main.quick_select
    );
    let signal = &state.device().signal;
    println!(
        "STATE video in={:?} out={:?}",
        signal.video.input, signal.video.output
    );
    println!(
        "STATE hdr in={:?} out={:?}",
        signal.hdr.input, signal.hdr.output
    );
    println!(
        "STATE color_space in={:?} out={:?}",
        signal.color_space.input, signal.color_space.output
    );
    println!(
        "STATE pixel_depth in={:?} out={:?}",
        signal.pixel_depth.input, signal.pixel_depth.output
    );
    println!(
        "STATE channels in={} out={}",
        signal
            .channels
            .input
            .map_or_else(|| "-".to_owned(), |c| c.to_string()),
        signal
            .channels
            .output
            .map_or_else(|| "-".to_owned(), |c| c.to_string()),
    );
    println!(
        "STATE audio_signal={:?} audio_sound={:?} sampling_rate={:?}",
        signal.audio_signal, signal.audio_sound, signal.sampling_rate
    );
    println!("STATE allm={:?} qft={:?}", signal.allm, signal.qft);
    println!(
        "STATE toggles tone_control={:?} dynamic_eq={:?} sound_modes={}",
        state.device().tone_control,
        state.device().dynamic_eq,
        state.device().sound_modes.as_slice().len(),
    );
}
