//! A fake HEOS device: a TCP server that answers with what a test says.
//!
//! Addresses, ids and titles in the answers are placeholders.

#![allow(dead_code)] // Each test file uses a part of this.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use denon_marantz_heos::{Change, Heos, HeosConfig, PlayerId};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::broadcast;

/// Lines that an AVC-A10H sent.
pub(crate) const DATA: &str =
    include_str!("../../../denon-marantz-heos-proto/tests/data/a10h.jsonl");

/// The player in [`DATA`].
pub(crate) const PID: PlayerId = PlayerId(1_234_567);

/// The IP address of the player in [`DATA`].
pub(crate) const IP: &str = "192.168.1.20";

/// Name of the command that registers for change events.
pub(crate) const REGISTER: &str = "system/register_for_change_events";

/// The address that the fakes listen on.
pub(crate) const LOOPBACK: &str = "127.0.0.1:0";

/// The IPv6 address that the fakes can listen on.
pub(crate) const LOOPBACK_V6: &str = "[::1]:0";

/// Short times, so that tests are quick.
pub(crate) const fn config(port: u16) -> HeosConfig {
    HeosConfig {
        port,
        connect_timeout: Duration::from_millis(500),
        write_timeout: Duration::from_millis(500),
        command_timeout: Duration::from_millis(500),
        slow_command_limit: Duration::from_millis(1200),
        keep_alive: Duration::from_secs(10),
        reconnect_min: Duration::from_millis(50),
        reconnect_max: Duration::from_millis(200),
        now_playing_delay: Duration::from_millis(50),
    }
}

/// The first answer in [`DATA`] to a command, for example
/// `player/get_play_state`. "Command under process" lines are skipped.
pub(crate) fn answer(command: &str) -> String {
    answer_lines(command).pop().unwrap()
}

/// The lines from [`DATA`] up to the first answer to a command: "command
/// under process" lines, then the answer.
pub(crate) fn answer_lines(command: &str) -> Vec<String> {
    let needle = format!("\"command\": \"{command}\"");
    let mut lines = Vec::new();
    for line in DATA.lines().filter(|line| line.contains(&needle)) {
        lines.push(line.to_owned());
        if !line.contains("command under process") {
            return lines;
        }
    }
    panic!("no answer to {command}");
}

/// The answer from [`DATA`] to `get_now_playing_media` with this song. An
/// empty song gives the `AirPlay` item without text.
pub(crate) fn now_playing(song: &str) -> String {
    let needle = format!("\"song\": \"{song}\"");
    DATA.lines()
        .find(|line| line.contains("get_now_playing_media") && line.contains(&needle))
        .unwrap_or_else(|| panic!("no now playing with {song:?}"))
        .to_owned()
}

/// Counts the calls of the returned function. The first call gives 0.
pub(crate) fn counter() -> impl Fn() -> usize + Send + Sync {
    let count = AtomicUsize::new(0);
    move || count.fetch_add(1, Ordering::SeqCst)
}

/// An event line.
pub(crate) fn event(name: &str, message: &str) -> String {
    format!(r#"{{"heos": {{"command": "event/{name}", "message": "{message}"}}}}"#)
}

/// A success answer that repeats the arguments of the command line in its
/// message, as the device does.
pub(crate) fn echo(line: &str) -> String {
    let message = line.split_once('?').map_or("", |(_, arguments)| arguments);
    format!(
        r#"{{"heos": {{"command": "{}", "result": "success", "message": "{message}"}}}}"#,
        name(line)
    )
}

/// A "command under process" answer.
pub(crate) fn under_process(command: &str) -> String {
    format!(
        r#"{{"heos": {{"command": "{command}", "result": "success", "message": "command under process"}}}}"#
    )
}

/// A failed answer.
pub(crate) fn failure(command: &str, message: &str) -> String {
    format!(r#"{{"heos": {{"command": "{command}", "result": "fail", "message": "{message}"}}}}"#)
}

/// Command name of a command line, for example `player/get_play_state`.
pub(crate) fn name(line: &str) -> &str {
    let path = line.strip_prefix("heos://").unwrap_or(line);
    path.split_once('?').map_or(path, |(name, _)| name)
}

/// What the fake device does with a command.
pub(crate) enum Reply {
    /// Send these lines.
    Lines(Vec<String>),
    /// Send these lines, with a pause before each one.
    Paced(Duration, Vec<String>),
    /// Never answer.
    Hang,
}

/// The answer from [`DATA`] to every command.
pub(crate) fn from_data(line: &str) -> Reply {
    Reply::Lines(answer_lines(name(line)))
}

/// Registers for change events that the fake refuses.
#[derive(Default)]
struct Refusals {
    /// How many registers in a row are refused.
    count: AtomicUsize,
    /// Wait before the refusal is sent.
    delay: Mutex<Duration>,
}

pub(crate) struct Fake {
    pub(crate) port: u16,
    seen: Arc<Mutex<Vec<String>>>,
    connections: Arc<AtomicUsize>,
    closed: Arc<AtomicUsize>,
    refusals: Arc<Refusals>,
    events: broadcast::Sender<String>,
    close: broadcast::Sender<()>,
}

impl Fake {
    pub(crate) async fn start(handler: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Self {
        Self::start_on(LOOPBACK, handler).await
    }

    /// Like [`Fake::start`], on the address `bind`.
    pub(crate) async fn start_on(
        bind: &str,
        handler: impl Fn(&str) -> Reply + Send + Sync + 'static,
    ) -> Self {
        let listener = TcpListener::bind(bind).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let fake = Self {
            port,
            seen: Arc::default(),
            connections: Arc::default(),
            closed: Arc::default(),
            refusals: Arc::default(),
            events: broadcast::Sender::new(64),
            close: broadcast::Sender::new(4),
        };
        let handler = Arc::new(handler);
        let (seen, connections, closed, refusals) = (
            fake.seen.clone(),
            fake.connections.clone(),
            fake.closed.clone(),
            fake.refusals.clone(),
        );
        let (events, close) = (fake.events.clone(), fake.close.clone());
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                connections.fetch_add(1, Ordering::SeqCst);
                let served = serve(
                    socket,
                    handler.clone(),
                    seen.clone(),
                    refusals.clone(),
                    events.clone(),
                    close.subscribe(),
                );
                let closed = closed.clone();
                tokio::spawn(async move {
                    served.await;
                    closed.fetch_add(1, Ordering::SeqCst);
                });
            }
        });
        fake
    }

    /// Command lines that were received, without `\r\n`.
    pub(crate) fn seen(&self) -> Vec<String> {
        self.seen.lock().unwrap().clone()
    }

    /// How many command lines with this command name were received.
    pub(crate) fn count(&self, command: &str) -> usize {
        self.seen
            .lock()
            .unwrap()
            .iter()
            .filter(|line| name(line) == command)
            .count()
    }

    /// How many connections were opened.
    pub(crate) fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }

    /// How many connections were closed.
    pub(crate) fn closed(&self) -> usize {
        self.closed.load(Ordering::SeqCst)
    }

    /// Sends a line on every connection that registered for events.
    pub(crate) fn push(&self, line: String) {
        self.events.send(line).unwrap();
    }

    /// Refuses the next `count` registers for change events.
    pub(crate) fn refuse_registers(&self, count: usize) {
        self.refusals.count.store(count, Ordering::SeqCst);
    }

    /// Waits this long before each refusal of a register.
    pub(crate) fn delay_refusals(&self, delay: Duration) {
        *self.refusals.delay.lock().unwrap() = delay;
    }

    /// Closes all open connections.
    pub(crate) fn close_all(&self) {
        let _ = self.close.send(());
    }
}

async fn serve(
    socket: TcpStream,
    handler: Arc<dyn Fn(&str) -> Reply + Send + Sync>,
    seen: Arc<Mutex<Vec<String>>>,
    refusals: Arc<Refusals>,
    events: broadcast::Sender<String>,
    mut close: broadcast::Receiver<()>,
) {
    let (read, mut write) = socket.into_split();
    let mut lines = BufReader::new(read).lines();
    let mut registered: Option<broadcast::Receiver<String>> = None;
    loop {
        let event = async {
            match registered.as_mut() {
                Some(receiver) => receiver.recv().await.ok(),
                None => std::future::pending().await,
            }
        };
        let mut register = false;
        let mut pause = Duration::ZERO;
        let out = tokio::select! {
            _ = close.recv() => return,
            line = lines.next_line() => {
                let Ok(Some(line)) = line else {
                    return;
                };
                let line = line.trim_end_matches('\r').to_owned();
                seen.lock().unwrap().push(line.clone());
                if name(&line) == REGISTER {
                    let refused = refusals
                        .count
                        .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| left.checked_sub(1))
                        .is_ok();
                    if refused {
                        pause = *refusals.delay.lock().unwrap();
                        vec![failure(name(&line), "eid=12&text=System error&syserrno=-9")]
                    } else {
                        register = true;
                        vec![answer(name(&line))]
                    }
                } else {
                    match handler(&line) {
                        Reply::Lines(lines) => lines,
                        Reply::Paced(wait, lines) => {
                            pause = wait;
                            lines
                        }
                        Reply::Hang => Vec::new(),
                    }
                }
            }
            Some(line) = event => vec![line],
        };
        if register {
            registered = Some(events.subscribe());
        }
        for line in out {
            tokio::time::sleep(pause).await;
            if write
                .write_all(format!("{line}\r\n").as_bytes())
                .await
                .is_err()
            {
                return;
            }
        }
    }
}

/// Connects to the fake device with [`config`].
pub(crate) async fn connect(fake: &Fake) -> Heos {
    Heos::connect_with("127.0.0.1", config(fake.port))
        .await
        .unwrap()
}

/// Waits for the next change. Panics after two seconds.
pub(crate) async fn next_change(changes: &mut broadcast::Receiver<Change>) -> Change {
    tokio::time::timeout(Duration::from_secs(2), changes.recv())
        .await
        .expect("no change")
        .unwrap()
}

/// Waits until the fake has seen `count` registers for change events.
/// Connect does not wait for the event connection, so call this before a
/// test changes what the fake does with a register.
pub(crate) async fn wait_for_registers(fake: &Fake, count: usize) {
    wait_until(|| fake.count(REGISTER) == count).await;
}

/// Waits until `done` is true. Panics after two seconds.
pub(crate) async fn wait_until(mut done: impl FnMut() -> bool) {
    for _ in 0..200 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("timed out");
}

/// A local port that refuses connections. Test servers bind port 0 and get a
/// port from the ephemeral range, so no test can take this port.
pub(crate) const CLOSED_PORT: u16 = 1;

/// Collects the log output of a test.
#[derive(Clone, Default)]
pub(crate) struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for LogBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl LogBuffer {
    /// Collects the logs that `filter` lets through, until the guard is
    /// dropped. Only for a test runtime with one thread: the subscriber is
    /// set for this thread, so tasks on other threads do not use it.
    pub(crate) fn capture(filter: &str) -> (Self, impl Sized) {
        use tracing_subscriber::util::SubscriberInitExt;
        // With only one dispatcher, `tracing` asks the dispatcher of the
        // thread that first logs at a place in the code (a callsite) if that
        // place is on. A test thread without a dispatcher then turns the place
        // off for all threads. A second dispatcher that stays alive stops this.
        static SECOND: std::sync::OnceLock<tracing::Dispatch> = std::sync::OnceLock::new();
        SECOND.get_or_init(|| tracing::Dispatch::new(tracing::subscriber::NoSubscriber::new()));
        let logs = Self::default();
        let writer = logs.clone();
        let guard = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(move || writer.clone())
            .finish()
            .set_default();
        (logs, guard)
    }

    pub(crate) fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap()).into_owned()
    }

    /// The span `name{...}` in the first line, after line `after`, that
    /// contains `what`.
    pub(crate) fn span_of(&self, what: &str, after: usize, name: &str) -> String {
        let text = self.text();
        let line = text
            .lines()
            .skip(after)
            .find(|line| line.contains(what))
            .unwrap_or_else(|| panic!("no line with {what:?} in:\n{text}"));
        let open = format!("{name}{{");
        let start = line
            .find(&open)
            .unwrap_or_else(|| panic!("no {name} span in: {line}"));
        line[start..].split('}').next().unwrap().to_owned()
    }

    pub(crate) fn line_count(&self) -> usize {
        self.text().lines().count()
    }
}
