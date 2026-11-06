//! Telnet connection on port 23: live events from the receiver.
//!
//! A receiver allows one telnet connection at a time. `Avr::telnet_connect`
//! opens it and keeps it open in a background task. The task reads events into
//! the state, sends a query when the connection is idle, and connects again
//! when the connection is lost.

use std::future::Future;
use std::time::Duration;

use denon_marantz_proto::telnet::{
    DEVICE_POWER_QUERY, DuplicateFilter, Event, EventZone, LineDecoder, Message,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, broadcast, watch};
use tokio::task::JoinHandle;

use crate::channels::Channels;
use crate::error::Error;

/// Events that a slow subscriber can fall behind before it loses some.
const EVENT_CAPACITY: usize = 256;

/// Bytes read at once.
const READ_BUFFER: usize = 2048;

/// Settings of the telnet connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TelnetConfig {
    /// Port of the receiver.
    pub port: u16,
    /// Time allowed to open the connection.
    pub connect_timeout: Duration,
    /// Time without data after which a query is sent to keep the connection
    /// alive. If there is still nothing after the same time again, the
    /// connection counts as lost.
    pub keep_alive: Duration,
    /// Time to wait before the first try to connect again. It doubles with
    /// each failed try up to [`TelnetConfig::reconnect_max`].
    pub reconnect_min: Duration,
    /// Longest time to wait between two tries to connect again.
    pub reconnect_max: Duration,
}

impl Default for TelnetConfig {
    fn default() -> Self {
        Self {
            port: 23,
            connect_timeout: Duration::from_secs(2),
            keep_alive: Duration::from_secs(30),
            reconnect_min: Duration::from_millis(500),
            reconnect_max: Duration::from_secs(30),
        }
    }
}

impl TelnetConfig {
    pub(crate) fn check(&self) -> Result<(), Error> {
        if self.connect_timeout.is_zero()
            || self.keep_alive.is_zero()
            || self.reconnect_min.is_zero()
        {
            return Err(Error::InvalidConfig("telnet times must be above 0"));
        }
        if self.reconnect_min > self.reconnect_max {
            return Err(Error::InvalidConfig(
                "reconnect_min must not be above reconnect_max",
            ));
        }
        Ok(())
    }
}

/// State of the telnet connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TelnetStatus {
    /// There is no connection. A task that is running tries to connect again.
    Disconnected,
    /// The connection is open.
    Connected,
}

/// An event from the receiver, as it was sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelnetEvent {
    /// Zone of the event.
    pub zone: EventZone,
    /// Event.
    pub event: Event,
    /// Text after the event.
    pub parameter: Box<str>,
}

/// The telnet connection of one receiver.
#[derive(Debug)]
pub(crate) struct Telnet {
    config: TelnetConfig,
    events: broadcast::Sender<TelnetEvent>,
    status: watch::Sender<TelnetStatus>,
    /// The running task. The lock also stops two calls from connecting at once.
    task: Mutex<Option<Task>>,
}

/// A running task.
#[derive(Debug)]
struct Task {
    shutdown: watch::Sender<bool>,
    handle: JoinHandle<()>,
}

impl Telnet {
    pub(crate) fn new(config: TelnetConfig) -> Self {
        Self {
            config,
            events: broadcast::Sender::new(EVENT_CAPACITY),
            status: watch::Sender::new(TelnetStatus::Disconnected),
            task: Mutex::new(None),
        }
    }

    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<TelnetEvent> {
        self.events.subscribe()
    }

    pub(crate) fn watch_status(&self) -> watch::Receiver<TelnetStatus> {
        self.status.subscribe()
    }

    /// Opens the connection and starts the task. Does nothing if it runs.
    pub(crate) async fn connect(&self, host: &str, channels: &Channels) -> Result<(), Error> {
        let mut task = self.task.lock().await;
        if task.as_ref().is_some_and(|task| !task.handle.is_finished()) {
            return Ok(());
        }
        let stream = open(host, &self.config).await?;
        let (shutdown, shutdown_receiver) = watch::channel(false);
        let worker = Worker {
            host: host.to_owned(),
            config: self.config,
            channels: channels.clone(),
            events: self.events.clone(),
            status: self.status.clone(),
        };
        let handle = tokio::spawn(worker.run(stream, shutdown_receiver));
        *task = Some(Task { shutdown, handle });
        Ok(())
    }

    /// Stops the task and closes the connection.
    pub(crate) async fn disconnect(&self) {
        let task = self.task.lock().await.take();
        if let Some(mut task) = task {
            let _ = task.shutdown.send(true);
            let _ = (&mut task.handle).await;
        }
    }
}

impl Drop for Task {
    /// Stops the task if the receiver value goes away. The task then sets the
    /// status back to disconnected and closes the connection.
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Sets the status to disconnected when the task ends, also if it is aborted.
struct StatusGuard(watch::Sender<TelnetStatus>);

impl Drop for StatusGuard {
    fn drop(&mut self) {
        self.0.send_replace(TelnetStatus::Disconnected);
    }
}

/// Why a session ended.
enum End {
    Shutdown,
    Closed,
    Failed(std::io::Error),
    Silent,
}

impl std::fmt::Display for End {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shutdown => f.write_str("stopped"),
            Self::Closed => f.write_str("closed by the receiver"),
            Self::Failed(error) => write!(f, "read or write failed: {error}"),
            Self::Silent => f.write_str("no answer to the keep-alive query"),
        }
    }
}

struct Worker {
    host: String,
    config: TelnetConfig,
    channels: Channels,
    events: broadcast::Sender<TelnetEvent>,
    status: watch::Sender<TelnetStatus>,
}

impl Worker {
    async fn run(self, first: TcpStream, mut shutdown: watch::Receiver<bool>) {
        let _guard = StatusGuard(self.status.clone());
        let mut stream = Some(first);
        loop {
            let stream = match stream.take() {
                Some(stream) => stream,
                None => match self.reconnect(&mut shutdown).await {
                    Some(stream) => stream,
                    None => return,
                },
            };
            self.status.send_replace(TelnetStatus::Connected);
            tracing::debug!(host = %self.host, "telnet connected");
            let end = self.session(stream, &mut shutdown).await;
            self.status.send_replace(TelnetStatus::Disconnected);
            if matches!(end, End::Shutdown) {
                return;
            }
            tracing::debug!(host = %self.host, reason = %end, "telnet connection lost");
        }
    }

    /// Tries to connect until it works. `None` if the task is stopped.
    ///
    /// It waits before the first try: the receiver needs a moment to free
    /// its only telnet connection, and a receiver that closes each connection
    /// at once must not be asked in a tight loop.
    async fn reconnect(&self, shutdown: &mut watch::Receiver<bool>) -> Option<TcpStream> {
        let mut wait = self.config.reconnect_min;
        loop {
            until_stopped(shutdown, tokio::time::sleep(wait)).await?;
            match until_stopped(shutdown, open(&self.host, &self.config)).await? {
                Ok(stream) => return Some(stream),
                Err(error) => tracing::debug!(host = %self.host, %error, "telnet reconnect failed"),
            }
            wait = (wait * 2).min(self.config.reconnect_max);
        }
    }

    /// Reads events until the connection ends.
    async fn session(&self, mut stream: TcpStream, shutdown: &mut watch::Receiver<bool>) -> End {
        let mut decoder = LineDecoder::new();
        let mut filter = DuplicateFilter::new();
        let mut buffer = [0_u8; READ_BUFFER];
        let keep_alive = format!("{DEVICE_POWER_QUERY}\r");
        let mut asked = false;
        loop {
            let Some(read) = until_stopped(
                shutdown,
                tokio::time::timeout(self.config.keep_alive, stream.read(&mut buffer)),
            )
            .await
            else {
                return End::Shutdown;
            };
            match read {
                Ok(Ok(0)) => return End::Closed,
                Ok(Ok(count)) => {
                    asked = false;
                    decoder.decode(&buffer[..count], |line| self.handle_line(line, &mut filter));
                }
                Ok(Err(error)) => return End::Failed(error),
                Err(_) if asked => return End::Silent,
                Err(_) => {
                    asked = true;
                    tracing::trace!(host = %self.host, "telnet idle, sending a query");
                    if let Err(error) = stream.write_all(keep_alive.as_bytes()).await {
                        return End::Failed(error);
                    }
                }
            }
        }
    }

    fn handle_line(&self, line: &str, filter: &mut DuplicateFilter) {
        let Some(message) = Message::parse(line) else {
            return;
        };
        if !filter.accept(message.parameter) {
            tracing::trace!(
                event = message.event.code(),
                "telnet duplicate event ignored"
            );
            return;
        }
        // A line can have personal data, for example a title.
        #[cfg(feature = "sensitive-logs")]
        tracing::trace!(line, "telnet line");
        #[cfg(not(feature = "sensitive-logs"))]
        tracing::trace!(event = message.event.code(), "telnet event");

        self.channels
            .modify(|state, notify| notify(state.apply_message(&message)));
        if self.events.receiver_count() > 0 {
            // There is an error if the last subscriber left. That is fine.
            let _ = self.events.send(TelnetEvent {
                zone: message.zone,
                event: message.event,
                parameter: message.parameter.into(),
            });
        }
    }
}

/// Opens a connection to the receiver.
async fn open(host: &str, config: &TelnetConfig) -> Result<TcpStream, Error> {
    let request = || format!("telnet connect {host}:{}", config.port);
    match tokio::time::timeout(
        config.connect_timeout,
        TcpStream::connect((host, config.port)),
    )
    .await
    {
        Err(_) => Err(Error::Timeout {
            request: request(),
            limit: config.connect_timeout,
        }),
        Ok(Err(source)) => Err(Error::Network {
            request: request(),
            source: Box::new(source),
        }),
        Ok(Ok(stream)) => {
            // Commands are short. Do not wait to fill a packet.
            let _ = stream.set_nodelay(true);
            Ok(stream)
        }
    }
}

/// Runs `work`. `None` if the task is stopped first.
async fn until_stopped<T>(
    shutdown: &mut watch::Receiver<bool>,
    work: impl Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        biased;
        _ = shutdown.changed() => None,
        output = work => Some(output),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_valid() {
        assert!(TelnetConfig::default().check().is_ok());
    }

    #[test]
    fn settings_are_checked() {
        let default = TelnetConfig::default();
        let bad = [
            TelnetConfig {
                connect_timeout: Duration::ZERO,
                ..default
            },
            TelnetConfig {
                keep_alive: Duration::ZERO,
                ..default
            },
            TelnetConfig {
                reconnect_min: Duration::ZERO,
                ..default
            },
            TelnetConfig {
                reconnect_min: Duration::from_secs(60),
                ..default
            },
        ];
        for config in bad {
            assert!(
                matches!(config.check(), Err(Error::InvalidConfig(_))),
                "{config:?}"
            );
        }
    }
}
