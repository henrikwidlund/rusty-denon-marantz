//! Telnet connection on port 23: live events from the receiver, and commands.
//!
//! A receiver allows one telnet connection at a time. `Avr::telnet_connect`
//! opens it and keeps it open in a background task. The task reads events into
//! the state, sends commands and queries, sends a query when the connection is
//! idle, and connects again when the connection is lost.
//!
//! Commands from the caller go first. The queries that fill the state after a
//! connect are background work: they wait for the commands and for a short
//! pause after the last one. Writes keep a minimum time between them.

use std::collections::VecDeque;
use std::future::Future;
use std::time::Duration;

use denon_marantz_proto::telnet::{
    Command, DEVICE_POWER_QUERY, DuplicateFilter, Event, EventZone, LineDecoder,
    MAIN_ZONE_POWER_QUERY, Message, QueryGroup, group_queries, queries,
};
use denon_marantz_proto::{Brand, TUNER_QUERIES, Zone};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::channels::Channels;
use crate::error::Error;

/// Events that a slow subscriber can fall behind before it loses some.
const EVENT_CAPACITY: usize = 256;

/// Requests that wait for the task. A caller that sends more waits.
const REQUEST_CAPACITY: usize = 64;

/// Bytes read at once.
const READ_BUFFER: usize = 2048;

/// Settings of the telnet connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TelnetConfig {
    /// Port of the receiver.
    pub port: u16,
    /// Time allowed to open the connection.
    pub connect_timeout: Duration,
    /// Time allowed to write one command.
    pub write_timeout: Duration,
    /// Time without data after which a query is sent to keep the connection
    /// alive. If there is still nothing after the same time again, the
    /// connection counts as lost.
    pub keep_alive: Duration,
    /// Time to wait before the first try to connect again. It doubles with
    /// each failed try up to [`TelnetConfig::reconnect_max`].
    pub reconnect_min: Duration,
    /// Longest time to wait between two tries to connect again.
    pub reconnect_max: Duration,
    /// Shortest time between a write and the next command from the caller.
    /// Receivers need at least 50 ms, and buffer commands that come faster.
    pub command_interval: Duration,
    /// Shortest time between a write and the next query that fills the state.
    /// A receiver answers a query with several events, so queries are sent
    /// slower than commands. Then a command does not wait in the receiver
    /// behind many queries.
    pub query_interval: Duration,
    /// Time without data from the receiver that a query waits for. The receiver
    /// answers in order, so a command that is sent while it still answers
    /// queries is answered late. A query does not wait longer than
    /// one second after the last write.
    pub query_quiet: Duration,
    /// Time after a command from the caller in which queries wait, so that the
    /// receiver answers the command first.
    pub background_pause: Duration,
    /// The groups of queries that fill the state after a connect, in the order
    /// that they are sent. A group that is left out stays empty until the
    /// receiver reports a change, or until it is fetched with
    /// [`Avr::telnet_refresh_group`](crate::Avr::telnet_refresh_group). The
    /// default is [`QueryGroup::DEFAULT_ORDER`].
    pub query_groups: &'static [QueryGroup],
}

impl Default for TelnetConfig {
    fn default() -> Self {
        Self {
            port: 23,
            connect_timeout: Duration::from_secs(2),
            write_timeout: Duration::from_secs(2),
            keep_alive: Duration::from_secs(30),
            reconnect_min: Duration::from_millis(500),
            reconnect_max: Duration::from_secs(30),
            command_interval: Duration::from_millis(50),
            query_interval: Duration::from_millis(50),
            query_quiet: Duration::from_millis(50),
            background_pause: Duration::from_millis(250),
            query_groups: QueryGroup::DEFAULT_ORDER,
        }
    }
}

impl TelnetConfig {
    pub(crate) fn check(&self) -> Result<(), Error> {
        let times = [
            self.connect_timeout,
            self.write_timeout,
            self.keep_alive,
            self.reconnect_min,
        ];
        if times.iter().any(Duration::is_zero) {
            return Err(Error::InvalidConfig(
                "connect_timeout, write_timeout, keep_alive and reconnect_min must be above 0",
            ));
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

/// A line to send. The caller waits for `done`.
struct Outgoing {
    line: String,
    done: Option<oneshot::Sender<Result<(), Error>>>,
}

impl Outgoing {
    /// Tells the caller that the line was not sent.
    fn fail(self) {
        if let Some(done) = self.done {
            let _ = done.send(Err(Error::NotConnected));
        }
    }
}

/// What the task is asked to do.
enum Request {
    Send(Outgoing),
    /// Ask the receiver for its state again.
    Refresh,
    /// Ask the receiver for one group of values.
    RefreshGroup(QueryGroup),
}

/// The telnet connection of one receiver.
#[derive(Debug)]
pub(crate) struct Telnet {
    config: TelnetConfig,
    queries: Queries,
    events: broadcast::Sender<TelnetEvent>,
    status: watch::Sender<TelnetStatus>,
    /// The running task. The lock also stops two calls from connecting at once.
    task: Mutex<Option<Task>>,
}

/// A running task.
#[derive(Debug)]
struct Task {
    shutdown: watch::Sender<bool>,
    requests: mpsc::Sender<Request>,
    handle: JoinHandle<()>,
}

/// The queries that ask for the state of the receiver.
#[derive(Debug, Clone, Copy)]
struct Queries {
    brand: Brand,
    /// A receiver with only the main zone reports its power on the device
    /// power event.
    single_zone: bool,
    /// The groups that the connection fetches, in this order.
    groups: &'static [QueryGroup],
}

impl Queries {
    fn all(self) -> VecDeque<&'static str> {
        let power = if self.single_zone {
            DEVICE_POWER_QUERY
        } else {
            MAIN_ZONE_POWER_QUERY
        };
        std::iter::once(power)
            .chain(queries(self.brand, self.groups))
            .collect()
    }
}

impl Telnet {
    pub(crate) fn new(config: TelnetConfig, brand: Brand, single_zone: bool) -> Self {
        Self {
            config,
            queries: Queries {
                brand,
                single_zone,
                groups: config.query_groups,
            },
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
        let (requests, request_receiver) = mpsc::channel(REQUEST_CAPACITY);
        let worker = Worker {
            host: host.to_owned(),
            config: self.config,
            queries: self.queries,
            channels: channels.clone(),
            events: self.events.clone(),
            status: self.status.clone(),
        };
        // The connection is open when this returns, so a command can follow.
        set_status(&self.status, TelnetStatus::Connected);
        let handle = tokio::spawn(worker.run(stream, request_receiver, shutdown_receiver));
        *task = Some(Task {
            shutdown,
            requests,
            handle,
        });
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

    /// Sends one line. Returns when it is written, or when it cannot be sent.
    pub(crate) async fn send(&self, line: String) -> Result<(), Error> {
        let (done, written) = oneshot::channel();
        self.request(Request::Send(Outgoing {
            line,
            done: Some(done),
        }))
        .await?;
        written.await.unwrap_or(Err(Error::NotConnected))
    }

    /// Asks the task to query the state of the receiver again.
    pub(crate) async fn refresh(&self) -> Result<(), Error> {
        self.request(Request::Refresh).await
    }

    /// Asks the task to query one group of values.
    pub(crate) async fn refresh_group(&self, group: QueryGroup) -> Result<(), Error> {
        self.request(Request::RefreshGroup(group)).await
    }

    /// Passes a request to the task. A request is refused while there is no
    /// connection: it would be sent late, after a connect.
    async fn request(&self, request: Request) -> Result<(), Error> {
        if *self.status.borrow() != TelnetStatus::Connected {
            return Err(Error::NotConnected);
        }
        let requests = {
            let task = self.task.lock().await;
            task.as_ref()
                .filter(|task| !task.handle.is_finished())
                .map(|task| task.requests.clone())
        };
        requests
            .ok_or(Error::NotConnected)?
            .send(request)
            .await
            .map_err(|_| Error::NotConnected)
    }
}

impl Drop for Task {
    /// Stops the task if the receiver value goes away. The task then sets the
    /// status back to disconnected and closes the connection.
    fn drop(&mut self) {
        self.handle.abort();
    }
}

/// Sets the status. Watchers are told only if it changed.
fn set_status(status: &watch::Sender<TelnetStatus>, new: TelnetStatus) {
    status.send_if_modified(|current| {
        let changed = *current != new;
        *current = new;
        changed
    });
}

/// Sets the status to disconnected when the task ends, also if it is aborted.
struct StatusGuard(watch::Sender<TelnetStatus>);

impl Drop for StatusGuard {
    fn drop(&mut self) {
        set_status(&self.0, TelnetStatus::Disconnected);
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
    queries: Queries,
    channels: Channels,
    events: broadcast::Sender<TelnetEvent>,
    status: watch::Sender<TelnetStatus>,
}

/// Longest time a query waits after the last write for the receiver to be quiet.
const MAX_QUERY_WAIT: Duration = Duration::from_secs(1);

/// Work that waits for a write.
struct Queue {
    /// Lines from the caller, and the keep-alive query.
    user: VecDeque<Outgoing>,
    /// Queries that fill the state.
    background: VecDeque<&'static str>,
    /// When the last line was written.
    last_write: Option<Instant>,
    /// When the last line from the caller was written.
    last_user_write: Option<Instant>,
    /// When data came from the receiver.
    last_read: Option<Instant>,
}

impl Queue {
    const fn new(background: VecDeque<&'static str>) -> Self {
        Self {
            user: VecDeque::new(),
            background,
            last_write: None,
            last_user_write: None,
            last_read: None,
        }
    }

    /// When the next line may be written. `None` if there is nothing to write.
    ///
    /// A line from the caller waits for the command interval after the last
    /// write. A query waits for the query interval, and for the pause after
    /// the last line from the caller. It also waits until the receiver is quiet,
    /// but not longer than [`MAX_QUERY_WAIT`] after the last write.
    fn write_time(&self, config: &TelnetConfig) -> Option<Instant> {
        let after_last = |interval: Duration| {
            self.last_write
                .map_or_else(Instant::now, |written| written + interval)
        };
        if !self.user.is_empty() {
            Some(after_last(config.command_interval))
        } else if !self.background.is_empty() {
            let mut spaced = after_last(config.query_interval);
            if let Some(read) = self.last_read {
                let quiet = (read + config.query_quiet).min(after_last(MAX_QUERY_WAIT));
                spaced = spaced.max(quiet);
            }
            let after_command = self
                .last_user_write
                .map(|written| written + config.background_pause);
            Some(after_command.map_or(spaced, |time| time.max(spaced)))
        } else {
            None
        }
    }

    /// Tells every caller that is waiting that the line is not sent.
    fn fail_all(&mut self) {
        for outgoing in self.user.drain(..) {
            outgoing.fail();
        }
    }
}

impl Worker {
    async fn run(
        self,
        first: TcpStream,
        mut requests: mpsc::Receiver<Request>,
        mut shutdown: watch::Receiver<bool>,
    ) {
        let _guard = StatusGuard(self.status.clone());
        let mut stream = Some(first);
        loop {
            let stream = if let Some(stream) = stream.take() {
                stream
            } else {
                let Some(stream) = self.reconnect(&mut shutdown).await else {
                    return;
                };
                // Requests that came while there was no connection are late.
                drop_requests(&mut requests);
                stream
            };
            set_status(&self.status, TelnetStatus::Connected);
            tracing::debug!(host = %self.host, "telnet connected");
            let end = self.session(stream, &mut requests, &mut shutdown).await;
            set_status(&self.status, TelnetStatus::Disconnected);
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

    /// Reads events and writes lines until the connection ends.
    async fn session(
        &self,
        mut stream: TcpStream,
        requests: &mut mpsc::Receiver<Request>,
        shutdown: &mut watch::Receiver<bool>,
    ) -> End {
        let mut decoder = LineDecoder::new();
        let mut filter = DuplicateFilter::new();
        let mut queue = Queue::new(self.queries.all());
        let mut buffer = [0_u8; READ_BUFFER];
        let mut line = String::new();
        let keep_alive = format!("{DEVICE_POWER_QUERY}\r");
        // Only data from the receiver counts. Writes say nothing about it.
        let mut idle_since = Instant::now();
        let mut asked = false;
        let end = loop {
            let write_time = queue.write_time(&self.config);
            tokio::select! {
                biased;
                _ = shutdown.changed() => break End::Shutdown,
                read = stream.read(&mut buffer) => match read {
                    Ok(0) => break End::Closed,
                    Ok(count) => {
                        idle_since = Instant::now();
                        queue.last_read = Some(idle_since);
                        asked = false;
                        let mut tuner = false;
                        decoder.decode(&buffer[..count], |text| {
                            // The source of the main zone changed to the tuner.
                            tuner |= text == "SITUNER";
                            self.handle_line(text, &mut filter);
                        });
                        if tuner {
                            queue.background.extend(TUNER_QUERIES);
                        }
                    }
                    Err(error) => break End::Failed(error),
                },
                request = requests.recv() => match request {
                    Some(Request::Send(outgoing)) => queue.user.push_back(outgoing),
                    Some(Request::Refresh) => {
                        filter.clear();
                        queue.background = self.queries.all();
                    }
                    Some(Request::RefreshGroup(group)) => {
                        filter.clear();
                        queue.background.extend(group_queries(self.queries.brand, group));
                    }
                    None => break End::Shutdown,
                },
                () = tokio::time::sleep_until(idle_since + self.config.keep_alive) => {
                    if asked {
                        break End::Silent;
                    }
                    asked = true;
                    // Wait the same time again for the answer.
                    idle_since = Instant::now();
                    tracing::trace!(host = %self.host, "telnet idle, sending a query");
                    queue.user.push_back(Outgoing { line: keep_alive.clone(), done: None });
                }
                () = tokio::time::sleep_until(write_time.unwrap_or_else(Instant::now)), if write_time.is_some() => {
                    if let Err(error) = self.write_next(&mut stream, &mut queue, &mut line).await {
                        break End::Failed(error);
                    }
                }
            }
        };
        queue.fail_all();
        drop_requests(requests);
        end
    }

    /// Writes the next line: from the caller first, then a query.
    async fn write_next(
        &self,
        stream: &mut TcpStream,
        queue: &mut Queue,
        line: &mut String,
    ) -> Result<(), std::io::Error> {
        let (text, done, from_user) = if let Some(outgoing) = queue.user.pop_front() {
            (outgoing.line, outgoing.done, true)
        } else if let Some(query) = queue.background.pop_front() {
            line.clear();
            line.push_str(query);
            line.push('\r');
            (std::mem::take(line), None, false)
        } else {
            return Ok(());
        };
        let result =
            tokio::time::timeout(self.config.write_timeout, stream.write_all(text.as_bytes()))
                .await
                .unwrap_or_else(|_| Err(std::io::ErrorKind::TimedOut.into()));
        let now = Instant::now();
        queue.last_write = Some(now);
        if from_user {
            queue.last_user_write = Some(now);
        } else {
            // Keep the buffer for the next query.
            *line = text;
        }
        if let Some(done) = done {
            let _ = done.send(match &result {
                Ok(()) => Ok(()),
                Err(error) => Err(Error::Network {
                    request: format!("telnet send to {}", self.host),
                    source: Box::new(std::io::Error::new(error.kind(), error.to_string())),
                }),
            });
        }
        result
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

/// Tells the callers of waiting requests that nothing was sent.
fn drop_requests(requests: &mut mpsc::Receiver<Request>) {
    while let Ok(request) = requests.try_recv() {
        if let Request::Send(outgoing) = request {
            outgoing.fail();
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

/// Encodes a command for a zone of a receiver of this brand.
pub(crate) fn encode(brand: Brand, zone: Zone, command: &Command<'_>) -> Result<String, Error> {
    let mut line = String::new();
    command
        .write_line(brand, zone, &mut line)
        .map_err(Error::InvalidCommand)?;
    Ok(line)
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
                write_timeout: Duration::ZERO,
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
        // No spacing and no pause are allowed, for tests and fast receivers.
        let none = TelnetConfig {
            command_interval: Duration::ZERO,
            background_pause: Duration::ZERO,
            ..default
        };
        assert!(none.check().is_ok());
    }

    #[test]
    fn power_query_depends_on_the_zones() {
        let multi = Queries {
            brand: Brand::Denon,
            single_zone: false,
            groups: QueryGroup::DEFAULT_ORDER,
        }
        .all();
        assert_eq!(multi.front(), Some(&"ZM?"));
        let single = Queries {
            brand: Brand::Marantz,
            single_zone: true,
            groups: QueryGroup::DEFAULT_ORDER,
        }
        .all();
        assert_eq!(single.front(), Some(&"PW?"));
        assert_eq!(
            multi.len(),
            1 + queries(Brand::Denon, QueryGroup::DEFAULT_ORDER).count()
        );
        assert_eq!(
            single.len(),
            1 + queries(Brand::Marantz, QueryGroup::DEFAULT_ORDER).count()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn queue_orders_the_work() {
        let config = TelnetConfig {
            command_interval: Duration::from_millis(50),
            query_interval: Duration::from_millis(100),
            query_quiet: Duration::from_millis(50),
            background_pause: Duration::from_millis(250),
            ..TelnetConfig::default()
        };
        let ms = Duration::from_millis;
        let mut queue = Queue::new(VecDeque::from(["SI?"]));
        let start = Instant::now();
        // The first write is due at once.
        assert_eq!(queue.write_time(&config), Some(start));
        // A line from the caller goes first. It waits for the command interval.
        queue.user.push_back(Outgoing {
            line: "MVUP\r".to_owned(),
            done: None,
        });
        queue.last_write = Some(start);
        assert_eq!(queue.write_time(&config), Some(start + ms(50)));
        // A query waits for the longer query interval.
        queue.user.clear();
        assert_eq!(queue.write_time(&config), Some(start + ms(100)));
        // After a command, queries wait for the pause.
        queue.last_user_write = Some(start);
        assert_eq!(queue.write_time(&config), Some(start + ms(250)));
        // If the last write is later, the interval counts from there.
        queue.last_write = Some(start + ms(400));
        assert_eq!(queue.write_time(&config), Some(start + ms(500)));
        // A query waits until the receiver is quiet, but not for ever.
        queue.last_read = Some(start + ms(480));
        assert_eq!(queue.write_time(&config), Some(start + ms(530)));
        queue.last_read = Some(start + ms(1390));
        assert_eq!(queue.write_time(&config), Some(start + ms(1400)));
        queue.background.clear();
        assert_eq!(queue.write_time(&config), None);
    }
}
