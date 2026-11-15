//! A fake HEOS device: a TCP server that answers with what a test says.
//!
//! Addresses, ids and titles in the answers are placeholders.

#![allow(dead_code)] // Each test file uses a part of this.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use denon_marantz_heos::{HeosConfig, PlayerId};
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

/// Short times, so that tests are quick.
pub(crate) const fn config(port: u16) -> HeosConfig {
    HeosConfig {
        port,
        connect_timeout: Duration::from_millis(500),
        write_timeout: Duration::from_millis(500),
        command_timeout: Duration::from_millis(500),
        keep_alive: Duration::from_secs(10),
        reconnect_min: Duration::from_millis(50),
        reconnect_max: Duration::from_millis(200),
        now_playing_delay: Duration::from_millis(50),
    }
}

/// The first answer in [`DATA`] to a command, for example
/// `player/get_play_state`.
pub(crate) fn answer(command: &str) -> String {
    let needle = format!("\"command\": \"{command}\"");
    DATA.lines()
        .find(|line| line.contains(&needle))
        .unwrap_or_else(|| panic!("no answer to {command}"))
        .to_owned()
}

/// An event line.
pub(crate) fn event(name: &str, message: &str) -> String {
    format!(r#"{{"heos": {{"command": "event/{name}", "message": "{message}"}}}}"#)
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
    /// Never answer.
    Hang,
}

/// The answer from [`DATA`] to every command.
pub(crate) fn from_data(line: &str) -> Reply {
    Reply::Lines(vec![answer(name(line))])
}

pub(crate) struct Fake {
    pub(crate) port: u16,
    seen: Arc<Mutex<Vec<String>>>,
    connections: Arc<AtomicUsize>,
    events: broadcast::Sender<String>,
    close: broadcast::Sender<()>,
}

impl Fake {
    pub(crate) async fn start(handler: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let fake = Self {
            port,
            seen: Arc::default(),
            connections: Arc::default(),
            events: broadcast::Sender::new(64),
            close: broadcast::Sender::new(4),
        };
        let handler = Arc::new(handler);
        let (seen, connections) = (fake.seen.clone(), fake.connections.clone());
        let (events, close) = (fake.events.clone(), fake.close.clone());
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                connections.fetch_add(1, Ordering::SeqCst);
                tokio::spawn(serve(
                    socket,
                    handler.clone(),
                    seen.clone(),
                    events.clone(),
                    close.subscribe(),
                ));
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

    /// Sends a line on every connection that registered for events.
    pub(crate) fn push(&self, line: String) {
        self.events.send(line).unwrap();
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
        let out = tokio::select! {
            _ = close.recv() => return,
            line = lines.next_line() => {
                let Ok(Some(line)) = line else {
                    return;
                };
                let line = line.trim_end_matches('\r').to_owned();
                seen.lock().unwrap().push(line.clone());
                if name(&line) == "system/register_for_change_events" {
                    register = true;
                    vec![answer(name(&line))]
                } else {
                    match handler(&line) {
                        Reply::Lines(lines) => lines,
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
