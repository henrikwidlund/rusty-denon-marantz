//! Connects to a HEOS device, runs the commands from the standard input, one
//! per line, and prints what changes, until you press Ctrl-C.
//!
//! ```sh
//! cargo run -p denon-marantz-heos --example heos_remote -- 192.168.1.20
//! echo "preset 1" | cargo run -p denon-marantz-heos --example heos_remote -- 192.168.1.20
//! ```
//!
//! The commands go to the player with the IP address of the host, or else to
//! the first player. After each line the example prints `done N`.
//!
//! Commands: play, pause, stop, next, previous, mode REPEAT SHUFFLE (REPEAT
//! is `off`, `on_all` or `on_one`, SHUFFLE is on or off), queue [FIRST LAST],
//! queue-raw (prints the answer line), play-item QID, remove QID..., clear,
//! favorites, preset N, quick-selects, quick N, stream URL, input NAME (for
//! example `inputs/aux_in_1`).
//!
//! The output can have personal data, for example a title that plays.

mod common;

use denon_marantz_heos::{
    Command, Heos, PlayMode, PlayState, PlayerId, PlayerState, QueueId, RepeatMode,
};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::broadcast::error::RecvError;
use tracing_subscriber::EnvFilter;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();
    let host = std::env::args()
        .nth(1)
        .ok_or("usage: heos_remote <host or IP address of the HEOS device>")?;

    let heos = Heos::connect(host.as_str()).await?;
    let pid = host
        .parse()
        .ok()
        .and_then(|ip| heos.player_by_ip(ip))
        .or_else(|| heos.players().first().map(PlayerState::pid))
        .ok_or("the device has no players")?;
    let mut changes = heos.changes();
    common::print_players(&heos);
    println!("connected, player {pid}, waiting for commands");

    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut input_open = true;
    let mut done = 0_u32;
    loop {
        tokio::select! {
            change = changes.recv() => match change {
                Ok(change) => common::print_change(&heos, &change),
                Err(RecvError::Lagged(count)) => println!("lost {count} changes"),
                Err(RecvError::Closed) => break,
            },
            line = lines.next_line(), if input_open => {
                // After the input ends, the changes are still shown.
                let Some(line) = line? else {
                    input_open = false;
                    continue;
                };
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                if let Err(error) = run(&heos, pid, line).await {
                    println!("error   {line}: {error}");
                }
                done += 1;
                println!("done    {done}");
            }
            _ = tokio::signal::ctrl_c() => break,
        }
    }
    heos.close().await;
    Ok(())
}

async fn run(heos: &Heos, pid: PlayerId, line: &str) -> Result<(), Box<dyn std::error::Error>> {
    let (name, rest) = line.split_once(' ').unwrap_or((line, ""));
    let mut args = rest.split_whitespace();
    let mut arg = || args.next().ok_or("an argument is missing");
    match name {
        "play" => heos.set_play_state(pid, PlayState::Play).await?,
        "pause" => heos.set_play_state(pid, PlayState::Pause).await?,
        "stop" => heos.set_play_state(pid, PlayState::Stop).await?,
        "next" => heos.play_next(pid).await?,
        "previous" => heos.play_previous(pid).await?,
        "mode" => {
            let repeat = RepeatMode::from_name(arg()?).ok_or("REPEAT is off, on_all or on_one")?;
            let shuffle = match arg()? {
                "on" => true,
                "off" => false,
                _ => return Err("SHUFFLE is on or off".into()),
            };
            heos.set_play_mode(pid, PlayMode { repeat, shuffle })
                .await?;
        }
        "queue" => {
            let range = match (args.next(), args.next()) {
                (Some(first), Some(last)) => Some((first.parse()?, last.parse()?)),
                (None, None) => None,
                _ => return Err("give FIRST and LAST, or none".into()),
            };
            let page = heos.queue(pid, range).await?;
            println!("queue   {} items, count {:?}", page.items.len(), page.count);
            for item in &page.items {
                println!("item    {} {:?} {:?}", item.qid.0, item.song, item.artist);
            }
        }
        "queue-raw" => {
            let answer = heos.send(Command::GetQueue { pid, range: None }).await?;
            println!("answer  {}", answer.line());
        }
        "play-item" => heos.play_queue_item(pid, QueueId(arg()?.parse()?)).await?,
        "remove" => {
            let qids = args
                .map(|qid| qid.parse().map(QueueId))
                .collect::<Result<Vec<_>, _>>()?;
            heos.remove_from_queue(pid, &qids).await?;
        }
        "clear" => heos.clear_queue(pid).await?,
        "favorites" => {
            let page = heos.favorites(None).await?;
            println!(
                "favorites {} items, count {:?}",
                page.items.len(),
                page.count
            );
            for (index, item) in page.items.iter().enumerate() {
                println!("item    {} {:?}", index + 1, item.name);
            }
        }
        "preset" => heos.play_preset(pid, arg()?.parse()?).await?,
        "quick-selects" => {
            for slot in heos.quick_selects(pid).await? {
                println!("quick   {} {:?}", slot.id, slot.name);
            }
        }
        "quick" => heos.play_quick_select(pid, arg()?.parse()?).await?,
        "stream" => heos.play_stream(pid, rest.trim()).await?,
        "input" => heos.play_input(pid, None, arg()?).await?,
        _ => return Err("unknown command".into()),
    }
    Ok(())
}
