//! Telnet connection on port 23: live events from the receiver, and commands.
//!
//! `Avr::telnet_connect` opens the connection and keeps it open in a
//! background task. The task reads events into the state, sends commands and
//! queries, sends a query when the connection is idle, and connects again when
//! the connection is lost.
//!
//! Commands from the caller go first. The queries that fill the state after a
//! connect are background work: they wait for the commands and for a short
//! pause after the last one. Writes keep a minimum time between them.

use std::borrow::Cow;
use std::collections::VecDeque;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use denon_marantz_proto::telnet::{
    AnswerLine, AnswerTracker, DEVICE_POWER_QUERY, DuplicateFilter, Event, EventZone, LineDecoder,
    MAIN_ZONE_POWER_QUERY, Message, Query, QueryGroup, SignalPart, group_queries, queries,
    signal_queries,
};
use denon_marantz_proto::{Brand, DAB_QUERIES, HD_RADIO_QUERIES, Power, TUNER_QUERIES, Zone};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::avr::{Info, Offered};
use crate::channels::Channels;
use crate::error::Error;
use crate::host::UrlHost;

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
    /// queries is answered late. A query does not wait longer than one second
    /// after the last write or, if that is later, after it was added.
    pub query_quiet: Duration,
    /// Time without an answer after which the queries that are still open
    /// count as not answered, see [`Avr::telnet_sync`](crate::Avr::telnet_sync).
    /// Some receivers never answer some queries. The time counts from the
    /// last answer or the last query, whichever is later.
    pub answer_quiet: Duration,
    /// Shortest wait before the video signal is asked after a hint. Some
    /// receivers do not send video signal changes. When the signal changes,
    /// they send the sound mode (`MS`) or `SSINFAISSIG`, also with the same
    /// value, or a new audio sample rate (`SSINFAISFSV`). After the first such
    /// line, the video signal and channel queries are sent after a random wait
    /// between this and [`TelnetConfig::signal_wait_max`]. Answers to queries
    /// of this connection are not hints. A line with a known input resolution
    /// in the meantime cancels the video queries, for example when another
    /// client asked. If that client asked only for the resolution, the other
    /// values can stay old. The channel queries are still sent. The random wait
    /// makes it less likely that several clients ask at the same time. When
    /// the answer has no input resolution yet (`---`), the video queries are
    /// sent once more after 1 s. Some changes send none of these lines, see
    /// [`TelnetConfig::signal_check`].
    pub signal_wait_min: Duration,
    /// Longest wait before the signal is asked after a hint, see
    /// [`TelnetConfig::signal_wait_min`].
    pub signal_wait_max: Duration,
    /// Time between checks of the signal while the main zone is on. Some
    /// receivers do not send all changes of the audio and video signal. A
    /// check asks the input resolution, HDR, audio signal, sample rate, sound
    /// and channels. A query is left out when its line came since the last
    /// check and this connection did not ask for it, for example because
    /// another client asked. If the resolution or HDR changed, all video
    /// signal queries are sent. If another client asked only for the
    /// resolution, the other video values can stay old. Only receivers with
    /// `CommApiVers` 0301 or higher get checks, see
    /// [`Info::has_heos`](crate::Info::has_heos). The main zone counts as on
    /// when the state says so, for example after its power line came. The
    /// video queries are left out while a video read waits for its write or
    /// its answers. A query that was not answered three times in a row is
    /// left out until a line for it comes. It must be above
    /// [`TelnetConfig::answer_quiet`], so that a query usually counts as not
    /// answered before the next check. A random wait of up to 0.5 s is added
    /// to each time. `None` turns the checks off. The default is 5 s.
    pub signal_check: Option<Duration>,
    /// The groups of queries that fill the state after a connect, in the order
    /// that they are sent. A group that is left out stays empty until the
    /// receiver reports a change, or until it is fetched with
    /// [`Avr::telnet_refresh_group`](crate::Avr::telnet_refresh_group). The
    /// default is [`QueryGroup::START_ORDER`]. [`QueryGroup::DEFAULT_ORDER`]
    /// has all groups. A start of a receiver that was read with `AppCommand`
    /// asks its own list first, and then the groups here that are not in
    /// [`QueryGroup::START_ORDER`].
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
            answer_quiet: Duration::from_secs(2),
            signal_wait_min: Duration::from_millis(300),
            signal_wait_max: Duration::from_millis(1200),
            signal_check: Some(Duration::from_secs(5)),
            query_groups: QueryGroup::START_ORDER,
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
            self.answer_quiet,
        ];
        if times.iter().any(Duration::is_zero) {
            return Err(Error::InvalidConfig(
                "connect_timeout, write_timeout, keep_alive, reconnect_min and answer_quiet must be above 0",
            ));
        }
        if self.reconnect_min > self.reconnect_max {
            return Err(Error::InvalidConfig(
                "reconnect_min must not be above reconnect_max",
            ));
        }
        if self.signal_wait_min > self.signal_wait_max {
            return Err(Error::InvalidConfig(
                "signal_wait_min must not be above signal_wait_max",
            ));
        }
        if self
            .signal_check
            .is_some_and(|every| every <= self.answer_quiet)
        {
            return Err(Error::InvalidConfig(
                "signal_check must be above answer_quiet",
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

impl TelnetStatus {
    /// The connection is open.
    pub(crate) fn is_connected(self) -> bool {
        self == Self::Connected
    }
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

/// What to ask a receiver for on telnet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Ask<'a> {
    /// A value that the library knows. The query depends on the brand.
    Query(Query),
    /// A query as text, for example `PSDYNEQ ?`, for a query that the library
    /// does not know, such as one of an older or a newer model. It must end
    /// with `?` and have no control character.
    Raw(&'a str),
}

impl From<Query> for Ask<'_> {
    fn from(query: Query) -> Self {
        Self::Query(query)
    }
}

/// The error for a line that may have been written, at least in part, before
/// the connection ended.
fn maybe_written(request: String) -> Error {
    Error::Network {
        request,
        source: Box::new(std::io::Error::other(
            "the connection ended while the line was sent",
        )),
        sent: true,
    }
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
    /// Ask the receiver these queries.
    Queries(Vec<Cow<'static, str>>),
    /// Tell the caller when the receiver has answered the queries, or was
    /// quiet for too long, or at `deadline`, see [`SyncResult`].
    /// `start` is true for the wait of the start, see [`Queue::start_missed`].
    Synced {
        done: oneshot::Sender<Result<SyncResult, Error>>,
        deadline: Instant,
        start: bool,
    },
}

/// How a wait for the answers ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SyncResult {
    /// No query waits for a write and none for its answer. Gives the queries
    /// that were not answered.
    Done(Vec<Cow<'static, str>>),
    /// The time limit came first. Gives the queries that were open then: not
    /// answered, written without an answer yet, lists without their `END`
    /// line, and not written yet.
    TimedOut(Vec<Cow<'static, str>>),
}

/// The telnet connection of one receiver.
#[derive(Debug)]
pub(crate) struct Telnet {
    config: TelnetConfig,
    /// See [`AvrBuilder::background_pause`](crate::AvrBuilder::background_pause).
    background_pause: Duration,
    queries: Queries,
    events: broadcast::Sender<TelnetEvent>,
    status: watch::Sender<TelnetStatus>,
    /// Set when the next event with the last value is a real change. The task
    /// then forgets the last values before it reads on.
    forget: Arc<AtomicBool>,
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
    /// Queries of values that the receiver does not offer are left out of
    /// the lists, see [`Info::offers`].
    offered: Offered,
    /// The tuner has DAB: the DAB queries go with the tuner queries.
    dab: bool,
    /// The receiver follows the current specs, see [`Info::has_heos`]. Only
    /// such receivers get signal checks.
    signal_check: bool,
}

impl Queries {
    fn new(groups: &'static [QueryGroup], info: &Info) -> Self {
        Self {
            brand: info.brand,
            single_zone: info.single_zone(),
            groups,
            offered: info.offered(),
            dab: info.has_dab(),
            signal_check: info.has_heos(),
        }
    }

    /// Returns false for a query of a value that the receiver does not
    /// offer, see [`Info::offers`].
    fn wanted(self, query: &str) -> bool {
        self.offered.offers_text(query)
    }

    fn all(self) -> VecDeque<Cow<'static, str>> {
        let power = if self.single_zone {
            DEVICE_POWER_QUERY
        } else {
            MAIN_ZONE_POWER_QUERY
        };
        std::iter::once(power)
            .chain(queries(self.brand, self.groups).filter(|query| self.wanted(query)))
            .map(Cow::Borrowed)
            .collect()
    }
}

impl Telnet {
    pub(crate) fn new(config: TelnetConfig, background_pause: Duration, info: &Info) -> Self {
        Self {
            config,
            background_pause,
            queries: Queries::new(config.query_groups, info),
            events: broadcast::Sender::new(EVENT_CAPACITY),
            status: watch::Sender::new(TelnetStatus::Disconnected),
            forget: Arc::new(AtomicBool::new(false)),
            task: Mutex::new(None),
        }
    }

    /// The connection asks for this group after a connect.
    pub(crate) fn fetches(&self, group: QueryGroup) -> bool {
        self.query_groups().contains(&group)
    }

    /// The groups that the connection asks for after a connect.
    pub(crate) const fn query_groups(&self) -> &'static [QueryGroup] {
        self.config.query_groups
    }

    /// The flag of [`Telnet::forget_values`], for a task that has no
    /// [`Telnet`].
    pub(crate) fn forget_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.forget)
    }

    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<TelnetEvent> {
        self.events.subscribe()
    }

    pub(crate) fn watch_status(&self) -> watch::Receiver<TelnetStatus> {
        self.status.subscribe()
    }

    /// Returns true if the connection is open now.
    pub(crate) fn is_connected(&self) -> bool {
        self.status.borrow().is_connected()
    }

    /// Opens the connection and starts the task. Does nothing if the task
    /// runs, also while it tries to connect again.
    ///
    /// `first` replaces the queries of the first session: the groups of the
    /// settings are not asked then, unless the caller put them in `first`. A
    /// session after a lost connection asks for the groups of the settings
    /// again.
    ///
    /// Returns true if this call opened the connection, and false if a task
    /// already runs. A running task keeps its queries.
    pub(crate) async fn connect(
        &self,
        host: &str,
        channels: &Channels,
        first: Option<Vec<&'static str>>,
    ) -> Result<bool, Error> {
        let mut task = self.task.lock().await;
        if task.as_ref().is_some_and(|task| !task.handle.is_finished()) {
            return Ok(false);
        }
        let stream = open(host, &self.config).await?;
        let (shutdown, shutdown_receiver) = watch::channel(false);
        let (requests, request_receiver) = mpsc::channel(REQUEST_CAPACITY);
        let worker = Worker {
            host: host.to_owned(),
            config: self.config,
            background_pause: self.background_pause,
            queries: self.queries,
            first: first.map(|queries| queries.into_iter().map(Cow::Borrowed).collect()),
            channels: channels.clone(),
            events: self.events.clone(),
            status: self.status.clone(),
            forget: self.forget.clone(),
        };
        // The connection is open when this returns, so a command can follow.
        set_status(&self.status, TelnetStatus::Connected);
        // The task owns the guard from the start, so the status is reset also
        // if the task is stopped before it runs.
        let guard = StatusGuard(self.status.clone());
        let handle = tokio::spawn(worker.run(stream, request_receiver, shutdown_receiver, guard));
        *task = Some(Task {
            shutdown,
            requests,
            handle,
        });
        Ok(true)
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
        // The task ended while the line waited or was written.
        written
            .await
            .unwrap_or_else(|_| Err(maybe_written("telnet send".to_owned())))
    }

    /// Asks the task to query the state of the receiver again.
    pub(crate) async fn refresh(&self) -> Result<(), Error> {
        self.request(Request::Refresh).await
    }

    /// Asks the task to query one group of values.
    pub(crate) async fn refresh_group(&self, group: QueryGroup) -> Result<(), Error> {
        self.request(Request::RefreshGroup(group)).await
    }

    /// Makes the task forget the last values that it saw, so that the next
    /// event with the old value is a real change. A page of the web interface
    /// has set values then. It does not wait, and it does nothing for a
    /// connection that opens later.
    pub(crate) fn forget_values(&self) {
        self.forget.store(true, Ordering::Relaxed);
    }

    /// The queries of the configured groups that are not in the start order.
    /// The start asks these after its own list.
    pub(crate) fn extra_start_queries(&self) -> Vec<&'static str> {
        let groups = self.queries.groups;
        groups
            .iter()
            .enumerate()
            .filter(|(index, group)| {
                !QueryGroup::START_ORDER.contains(group) && !groups[..*index].contains(group)
            })
            .flat_map(|(_, group)| group_queries(self.queries.brand, *group))
            .filter(|query| self.queries.wanted(query))
            .collect()
    }

    /// Returns false for a query of a value that the receiver does not
    /// offer, see [`Info::offers`].
    pub(crate) fn wanted(&self, query: &str) -> bool {
        self.queries.wanted(query)
    }

    /// Waits until no query waits for a write and each written query is
    /// answered, or counts as not answered after
    /// [`TelnetConfig::answer_quiet`]. Gives the queries that were not
    /// answered. It is an error if the connection ends first, or if this takes
    /// longer than the keep-alive time.
    pub(crate) async fn sync(&self) -> Result<Vec<Cow<'static, str>>, Error> {
        let limit = self.config.keep_alive;
        match self.wait_for_answers(limit, false).await? {
            SyncResult::Done(not_answered) => Ok(not_answered),
            SyncResult::TimedOut(_) => Err(Error::Timeout {
                request: "telnet answers".to_owned(),
                limit,
                sent: true,
            }),
        }
    }

    /// As `sync`, for the start, with another limit. Gives the open queries
    /// if the limit comes first. The queries that were not answered before
    /// this call count too.
    pub(crate) async fn sync_start(&self, limit: Duration) -> Result<SyncResult, Error> {
        self.wait_for_answers(limit, true).await
    }

    async fn wait_for_answers(&self, limit: Duration, start: bool) -> Result<SyncResult, Error> {
        let (done, answered) = oneshot::channel();
        let deadline = Instant::now() + limit;
        self.request(Request::Synced {
            done,
            deadline,
            start,
        })
        .await?;
        answered.await.unwrap_or(Err(Error::NotConnected))
    }

    /// Asks the task to send these queries after the ones that wait.
    pub(crate) async fn ask<Q: Into<Cow<'static, str>>>(
        &self,
        queries: impl IntoIterator<Item = Q>,
    ) -> Result<(), Error> {
        let queries = queries.into_iter().map(Into::into).collect();
        self.request(Request::Queries(queries)).await
    }

    /// Passes a request to the task. A request is refused while there is no
    /// connection: it would be sent late, after a connect.
    async fn request(&self, request: Request) -> Result<(), Error> {
        if !self.is_connected() {
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
    background_pause: Duration,
    queries: Queries,
    /// The queries of the first session, if they are not the ones of `queries`.
    first: Option<VecDeque<Cow<'static, str>>>,
    channels: Channels,
    events: broadcast::Sender<TelnetEvent>,
    status: watch::Sender<TelnetStatus>,
    forget: Arc<AtomicBool>,
}

/// Longest time a query waits after the last write for the receiver to be quiet.
const MAX_QUERY_WAIT: Duration = Duration::from_secs(1);

/// Work that waits for a write.
struct Queue {
    /// Lines from the caller, and the keep-alive query.
    user: VecDeque<Outgoing>,
    /// Queries that fill the state.
    background: VecDeque<Cow<'static, str>>,
    /// When the last line was written.
    last_write: Option<Instant>,
    /// When the last line from the caller was written.
    last_user_write: Option<Instant>,
    /// When data came from the receiver.
    last_read: Option<Instant>,
    /// When a query started to wait while nothing else waited.
    waiting_since: Option<Instant>,
    /// The written queries that are not answered yet.
    answers: AnswerTracker,
    /// When a query was written or a line answered one.
    last_answer: Instant,
    /// As `last_answer`, or when a line came that belongs to a query that
    /// was answered.
    last_activity: Instant,
    /// Callers that wait until the queries are answered.
    waiters: Vec<Waiter>,
    /// Queries that were not answered while no wait of the start waited,
    /// since the connection opened or since the last such wait began. The
    /// start begins its wait after its pages, so a query can count as not
    /// answered before.
    start_missed: Vec<Cow<'static, str>>,
    /// For each query of [`SIGNAL_CHECK`], how often in a row it was not
    /// answered, up to [`SIGNAL_CHECK_MISSES`].
    check_misses: [u8; SIGNAL_CHECK.len()],
    /// Queries that were last added by the connection itself, not by the
    /// caller: hint reads, checks and other follow-up reads. A miss of such a
    /// query is not reported to the waits.
    own: Vec<&'static str>,
}

/// A caller that waits until the queries are answered.
struct Waiter {
    done: oneshot::Sender<Result<SyncResult, Error>>,
    /// Queries that were not answered while it waited.
    missed: Vec<Cow<'static, str>>,
    /// When it stops waiting and gets the open queries.
    deadline: Instant,
    /// The wait of the start.
    start: bool,
}

/// When to ask the signal after a hint, see
/// [`TelnetConfig::signal_wait_min`], and when to check it, see
/// [`TelnetConfig::signal_check`].
#[derive(Debug, Default)]
struct SignalHint {
    /// When the signal queries are added.
    due: Option<Instant>,
    /// The parts that are asked at `due`.
    read: SignalRead,
    /// `due` is the second read after an answer without values.
    again_due: bool,
    /// The last read was the first one after a hint, and asked the video
    /// signal.
    may_ask_again: bool,
    /// The last audio sample rate that the receiver sent by itself. It
    /// repeats it every few seconds.
    sample_rate: String,
    /// When the signal is checked next, see [`TelnetConfig::signal_check`].
    check_at: Option<Instant>,
    /// Time between checks.
    check_every: Duration,
    /// The lines of [`SIGNAL_CHECK`] that came since the last check, and that
    /// this connection did not ask for.
    fresh: [bool; SIGNAL_CHECK.len()],
    /// The answer prefix of the last video query of a check, while its
    /// answers did not come yet.
    checking: Option<&'static str>,
}

impl SignalHint {
    /// Checks the signal every `check_every` plus `jitter`, or never if it
    /// is `None`.
    fn new(now: Instant, check_every: Option<Duration>, jitter: Duration) -> Self {
        Self {
            check_at: check_every.map(|every| now + every + jitter),
            check_every: check_every.unwrap_or_default(),
            ..Self::default()
        }
    }

    /// Checks a line from the receiver. `answer` says how the line belongs to
    /// the queries of this connection: only `Other` lines are hints. `wait`
    /// gives a random wait.
    fn line(
        &mut self,
        line: &str,
        answer: AnswerLine,
        now: Instant,
        wait: impl FnOnce() -> Duration,
    ) {
        // Only lines that the receiver sends by itself keep the sample rate.
        // A new sample rate in an answer to this connection is found when the
        // receiver repeats it.
        let hint = answer == AnswerLine::Other && self.is_hint(line);
        if answer == AnswerLine::Other
            && let Some(index) = signal_check_index(line)
        {
            self.fresh[index] = true;
        }
        match input_resolution(line).map(|value| matches!(value.trim(), "" | "---")) {
            // The answer to a check has only the resolution and HDR.
            Some(false) if answer == AnswerLine::Answer && self.checking.is_some() => {}
            Some(false) => {
                // Somebody asked, so the video values are new. The channels
                // are still asked.
                self.read.video = false;
                if !self.read.channels {
                    self.due = None;
                }
                self.may_ask_again = false;
            }
            // The answer came before the receiver had the new values.
            Some(true) => {
                if answer == AnswerLine::Answer && self.may_ask_again {
                    self.may_ask_again = false;
                    if self.due.is_none() {
                        self.due = Some(now + SIGNAL_AGAIN);
                        self.read.video = true;
                        self.again_due = true;
                    }
                }
            }
            None if hint => {
                self.read = SignalRead {
                    video: true,
                    channels: true,
                };
                // Later hints of the burst do not move the time.
                if self.due.is_none() {
                    self.due = Some(now + wait());
                    self.again_due = false;
                }
            }
            None => {}
        }
    }

    /// True for a sound mode, `SSINFAISSIG`, or a new audio sample rate.
    fn is_hint(&mut self, line: &str) -> bool {
        let Some(rate) = line.strip_prefix("SSINFAISFSV ") else {
            return line.starts_with("MS") || line.starts_with("SSINFAISSIG");
        };
        let rate = rate.trim();
        if self.sample_rate == rate {
            return false;
        }
        // The first value is no change.
        let changed = !self.sample_rate.is_empty();
        self.sample_rate.clear();
        self.sample_rate.push_str(rate);
        changed
    }

    /// Checks a line after the duplicate filter. `new` is true if the filter
    /// let it through. A changed answer to a check asks the video signal.
    fn check_line(&mut self, line: &str, answer: AnswerLine, new: bool, now: Instant) {
        let Some(last) = self.checking else {
            return;
        };
        if answer != AnswerLine::Answer
            || !SIGNAL_CHECK[..SIGNAL_CHECK_VIDEO]
                .iter()
                .any(|(_, prefix)| line.starts_with(prefix))
        {
            return;
        }
        if line.starts_with(last) || new {
            self.checking = None;
        }
        if new {
            self.read.video = true;
            self.due.get_or_insert(now);
        }
    }

    /// Takes the next check. Returns the queries of [`SIGNAL_CHECK`] whose
    /// line did not come since the last check. `jitter` is added to the next
    /// time.
    fn take_check(&mut self, now: Instant, jitter: Duration) -> [bool; SIGNAL_CHECK.len()] {
        self.check_at = Some(now + self.check_every + jitter);
        // A check whose answers did not come has ended.
        self.checking = None;
        let ask = self.fresh.map(|fresh| !fresh);
        self.fresh = [false; SIGNAL_CHECK.len()];
        ask
    }

    /// Takes the due time and the parts to ask when the queries are added.
    const fn take_due(&mut self) -> SignalRead {
        let read = self.read;
        self.due = None;
        // A video read gives the same answers.
        if read.video {
            self.checking = None;
        }
        self.may_ask_again = read.video && !self.again_due;
        self.read = SignalRead {
            video: false,
            channels: false,
        };
        self.again_due = false;
        read
    }
}

/// The parts of the signal that a hint asks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct SignalRead {
    /// The video signal.
    video: bool,
    /// The channels. Some receivers do not send the input channels by
    /// themselves when the audio signal changes.
    channels: bool,
}

/// The queries of a signal check, see [`TelnetConfig::signal_check`], and
/// the start of their answer lines. The video queries come first.
const SIGNAL_CHECK: [(&str, &str); 7] = [
    ("SSINFSIGRES ?", "SSINFSIGRES I"),
    ("SSINFSIGHDR ?", "SSINFSIGHDR I"),
    ("SSINFAISFSV ?", "SSINFAISFSV "),
    ("SYSDA ?", "SYSDA "),
    ("SYSMI ?", "SYSMI "),
    ("OPINFINS ?", "OPINFINS "),
    ("OPINFASP ?", "OPINFASP "),
];

/// The index in [`SIGNAL_CHECK`] of the query that `line` answers.
fn signal_check_index(line: &str) -> Option<usize> {
    SIGNAL_CHECK
        .iter()
        .position(|(_, prefix)| line.starts_with(prefix))
}

/// A query of [`SIGNAL_CHECK`] that was not answered this often in a row is
/// left out of the checks, until a line for it comes.
const SIGNAL_CHECK_MISSES: u8 = 3;

/// The number of video queries at the start of [`SIGNAL_CHECK`].
const SIGNAL_CHECK_VIDEO: usize = 2;

/// Longest random wait added to the time between signal checks.
const SIGNAL_CHECK_JITTER: Duration = Duration::from_millis(500);

/// Wait before the video signal is asked again after an answer without values.
const SIGNAL_AGAIN: Duration = Duration::from_secs(1);

/// The input resolution of a line, which every answer with the video values
/// has. Some receivers send `---` by themselves when the input signal is
/// lost, before the new values are known.
fn input_resolution(line: &str) -> Option<&str> {
    line.strip_prefix("SSINFSIGRES I")
        .or_else(|| line.strip_prefix("SYSDVIN "))
}

/// A random time from `min` to `max`.
fn random_wait(min: Duration, max: Duration) -> Duration {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let span = u64::try_from(max.saturating_sub(min).as_millis()).unwrap_or(u64::MAX);
    // Each `RandomState` has other keys, so the result changes.
    let random = RandomState::new().build_hasher().finish();
    min + Duration::from_millis(random % span.saturating_add(1))
}

/// Time without a line of an answer after which the answers count as complete.
/// Some answers have more lines, for example `TR?`, without an end line. Such
/// a line counts only this long after the last answer, so that the events of
/// other clients, for example a volume ramp, cannot keep a wait going.
const ANSWER_SETTLE: Duration = Duration::from_millis(300);

impl Queue {
    fn new(background: VecDeque<Cow<'static, str>>, brand: Brand) -> Self {
        let now = Instant::now();
        Self {
            waiting_since: (!background.is_empty()).then_some(now),
            user: VecDeque::new(),
            background,
            last_write: None,
            last_user_write: None,
            last_read: None,
            answers: AnswerTracker::new(brand),
            last_answer: now,
            last_activity: now,
            waiters: Vec::new(),
            start_missed: Vec::new(),
            check_misses: [0; SIGNAL_CHECK.len()],
            own: Vec::new(),
        }
    }

    /// Adds queries that the connection asks by itself, see [`Queue::own`].
    fn add_own(&mut self, queries: impl IntoIterator<Item = &'static str>) {
        for query in queries {
            // A query that the caller still waits for stays the caller's.
            let own = self.own.contains(&query) || !self.is_waiting(query);
            self.add([query]);
            if own {
                self.own.push(query);
            }
        }
    }

    /// True if `query` waits for its write or for its answer.
    fn is_waiting(&self, query: &str) -> bool {
        self.background.iter().any(|waiting| waiting == query)
            || self.answers.open().any(|open| open == query)
    }

    /// A query waits for a write.
    fn has_query_work(&self) -> bool {
        !self.background.is_empty()
    }

    /// Runs `change`, and notes the time if it made work wait while none did.
    fn queue_work(&mut self, change: impl FnOnce(&mut Self)) {
        let was_idle = !self.has_query_work();
        change(self);
        if was_idle && self.has_query_work() {
            self.waiting_since = Some(Instant::now());
        }
    }

    /// When the next line may be written. `None` if there is nothing to write.
    ///
    /// A line from the caller waits for the command interval after the last
    /// write. A query waits for the query interval, and for the pause after
    /// the last line from the caller. It also waits until the receiver is quiet,
    /// but not longer than [`MAX_QUERY_WAIT`] after the last write or, if that
    /// is later, after the query started to wait.
    fn write_time(&self, config: &TelnetConfig, background_pause: Duration) -> Option<Instant> {
        let after_last = |interval: Duration| {
            self.last_write
                .map_or_else(Instant::now, |written| written + interval)
        };
        if !self.user.is_empty() {
            Some(after_last(config.command_interval))
        } else if self.has_query_work() {
            let mut spaced = after_last(config.query_interval);
            if let Some(read) = self.last_read {
                let limit = self
                    .last_write
                    .max(self.waiting_since)
                    .map_or_else(Instant::now, |since| since + MAX_QUERY_WAIT);
                spaced = spaced.max((read + config.query_quiet).min(limit));
            }
            let after_command = self
                .last_user_write
                .map(|written| written + background_pause);
            Some(after_command.map_or(spaced, |time| time.max(spaced)))
        } else {
            None
        }
    }

    /// Notes what a line from the receiver means for the written queries.
    fn answer_line(&mut self, line: &str, now: Instant) -> AnswerLine {
        let kind = self.answers.line(line);
        match kind {
            AnswerLine::Answer => {
                self.last_answer = now;
                self.last_activity = now;
            }
            AnswerLine::Related if now < self.last_answer + ANSWER_SETTLE => {
                self.last_activity = now;
            }
            _ => {}
        }
        // The receiver knows the query.
        if kind != AnswerLine::Related
            && let Some(index) = signal_check_index(line)
        {
            self.check_misses[index] = 0;
        }
        kind
    }

    /// When [`Queue::check_answers`] has work. `None` if there is none. A
    /// caller that waits has no work while a query waits for a write: the
    /// write opens it. Its deadline is work too.
    fn answer_time(&self, quiet: Duration) -> Option<Instant> {
        let waits = !self.waiters.is_empty() && !self.has_query_work();
        let check = if !self.answers.is_idle() {
            Some(self.last_answer + quiet)
        } else if waits || self.answers.has_answered() {
            Some(self.last_activity + ANSWER_SETTLE)
        } else {
            None
        };
        let deadline = self.waiters.iter().map(|waiter| waiter.deadline).min();
        check.into_iter().chain(deadline).min()
    }

    /// Counts the open queries as not answered after the quiet time. Tells the
    /// callers that wait when no query waits and the answers are complete,
    /// or when their deadline came.
    fn check_answers(&mut self, now: Instant, quiet: Duration) {
        self.end_late_waiters(now);
        if !self.answers.is_idle() {
            if now < self.last_answer + quiet {
                return;
            }
            self.waiters.retain(|waiter| !waiter.done.is_closed());
            let mut missed: Vec<_> = self.answers.take_open().collect();
            tracing::debug!(queries = ?missed, "telnet queries not answered");
            for query in &missed {
                if let Some(index) = SIGNAL_CHECK.iter().position(|(text, _)| text == query) {
                    let count = &mut self.check_misses[index];
                    *count = count.saturating_add(1).min(SIGNAL_CHECK_MISSES);
                }
            }
            // The caller did not ask these. A query can be written twice, so
            // it leaves `own` only when no copy waits for its write any more.
            missed.retain(|query| !self.own.iter().any(|own| own == query));
            let background = &self.background;
            self.own
                .retain(|own| background.iter().any(|query| query == own));
            for waiter in &mut self.waiters {
                waiter.missed.extend(missed.iter().cloned());
            }
            if !self.waiters.iter().any(|waiter| waiter.start) {
                for query in missed {
                    if !self.start_missed.contains(&query) {
                        self.start_missed.push(query);
                    }
                }
            }
        }
        if now < self.last_activity + ANSWER_SETTLE {
            return;
        }
        self.answers.forget_answered();
        if self.has_query_work() {
            return;
        }
        for waiter in self.waiters.drain(..) {
            let _ = waiter.done.send(Ok(SyncResult::Done(waiter.missed)));
        }
    }

    /// Adds a caller that waits. The wait of the start begins with the
    /// queries that were not answered before it.
    fn add_waiter(
        &mut self,
        done: oneshot::Sender<Result<SyncResult, Error>>,
        deadline: Instant,
        start: bool,
    ) {
        let missed = if start {
            std::mem::take(&mut self.start_missed)
        } else {
            Vec::new()
        };
        self.waiters.push(Waiter {
            done,
            missed,
            deadline,
            start,
        });
    }

    /// Gives the callers whose deadline came the queries that are open:
    /// counted as not answered, written without an answer, and not written.
    /// The connection's own queries are left out, see [`Queue::own`].
    fn end_late_waiters(&mut self, now: Instant) {
        for waiter in self.waiters.extract_if(.., |waiter| waiter.deadline <= now) {
            let mut open = waiter.missed;
            for query in self.answers.open().chain(&self.background) {
                if !open.contains(query) && !self.own.iter().any(|own| own == query) {
                    open.push(query.clone());
                }
            }
            let _ = waiter.done.send(Ok(SyncResult::TimedOut(open)));
        }
    }

    /// Adds queries that wait for a write. A query that already waits is not
    /// added again, so repeated requests cannot make the queue grow without
    /// limit.
    fn add<Q: Into<Cow<'static, str>>>(&mut self, queries: impl IntoIterator<Item = Q>) {
        self.queue_work(|queue| {
            for query in queries {
                let query = query.into();
                // The caller wants its answer.
                queue.own.retain(|own| *own != query);
                if !queue.background.contains(&query) {
                    queue.background.push_back(query);
                }
            }
        });
    }

    /// Tells every caller that is waiting that the line is not sent.
    fn fail_all(&mut self) {
        for outgoing in self.user.drain(..) {
            outgoing.fail();
        }
        for waiter in self.waiters.drain(..) {
            let _ = waiter.done.send(Err(Error::NotConnected));
        }
    }
}

impl Worker {
    async fn run(
        mut self,
        first: TcpStream,
        mut requests: mpsc::Receiver<Request>,
        mut shutdown: watch::Receiver<bool>,
        _guard: StatusGuard,
    ) {
        let mut stream = Some(first);
        loop {
            let stream = if let Some(stream) = stream.take() {
                stream
            } else {
                let Some(stream) = self.reconnect(&mut shutdown, &mut requests).await else {
                    return;
                };
                // Requests that came while there was no connection are late.
                drop_requests(&mut requests);
                stream
            };
            set_status(&self.status, TelnetStatus::Connected);
            tracing::debug!(host = %self.host, "telnet connected");
            let background = self.first.take().unwrap_or_else(|| self.queries.all());
            let end = self
                .session(stream, background, &mut requests, &mut shutdown)
                .await;
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
    ///
    /// A request that comes in the meantime is refused at once, so that its
    /// caller does not wait for a connection that may not come.
    async fn reconnect(
        &self,
        shutdown: &mut watch::Receiver<bool>,
        requests: &mut mpsc::Receiver<Request>,
    ) -> Option<TcpStream> {
        let mut wait = self.config.reconnect_min;
        loop {
            until_stopped(
                shutdown,
                refusing_requests(requests, tokio::time::sleep(wait)),
            )
            .await?;
            let opened = until_stopped(
                shutdown,
                refusing_requests(requests, open(&self.host, &self.config)),
            )
            .await?;
            match opened {
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
        background: VecDeque<Cow<'static, str>>,
        requests: &mut mpsc::Receiver<Request>,
        shutdown: &mut watch::Receiver<bool>,
    ) -> End {
        let mut decoder = LineDecoder::new();
        let mut filter = DuplicateFilter::new();
        // A new connection has no last values to forget.
        self.forget.store(false, Ordering::Relaxed);
        let mut queue = Queue::new(background, self.queries.brand);
        let mut buffer = [0_u8; READ_BUFFER];
        let mut line = String::new();
        let keep_alive = format!("{DEVICE_POWER_QUERY}\r");
        // Only data from the receiver counts. Writes say nothing about it.
        let mut idle_since = Instant::now();
        let mut asked = false;
        let mut signal = self.signal_hint(idle_since);
        let (wait_min, wait_max) = (self.config.signal_wait_min, self.config.signal_wait_max);
        let end = loop {
            let write_time = queue.write_time(&self.config, self.background_pause);
            let answer_time = queue.answer_time(self.config.answer_quiet);
            tokio::select! {
                biased;
                _ = shutdown.changed() => break End::Shutdown,
                read = stream.read(&mut buffer) => match read {
                    Ok(0) => break End::Closed,
                    Ok(count) => {
                        if self.forget.swap(false, Ordering::Relaxed) {
                            filter.clear();
                        }
                        idle_since = Instant::now();
                        queue.last_read = Some(idle_since);
                        asked = false;
                        let mut tuner = false;
                        let mut hd_radio = false;
                        let unknown_before = self.colorimetry_unknown();
                        decoder.decode(&buffer[..count], |text| {
                            // The source of the main zone changed to the tuner.
                            tuner |= text == "SITUNER";
                            hd_radio |= text == "SIHDRADIO";
                            // Before the filter: a repeated value still answers,
                            // and is still a hint.
                            let answer = queue.answer_line(text, idle_since);
                            signal.line(text, answer, idle_since, || random_wait(wait_min, wait_max));
                            let new = self.handle_line(text, &mut filter);
                            signal.check_line(text, answer, new, idle_since);
                        });
                        self.add_source_queries(&mut queue, tuner, hd_radio);
                        // A change made BT.2020 not known: ask once more. The
                        // answer can repeat the last value, so the filter must
                        // let it through. This asks only when it becomes not
                        // known: if a query from HTTP failed before, it stays
                        // off until the next change or `refresh_signal`.
                        if !unknown_before && self.colorimetry_unknown() {
                            filter.forget_colorimetry();
                            queue.add_own(signal_queries(self.queries.brand, SignalPart::Colorimetry));
                        }
                    }
                    Err(error) => break End::Failed(error),
                },
                request = requests.recv() => match request {
                    Some(Request::Send(outgoing)) => queue.user.push_back(outgoing),
                    Some(Request::Refresh) => {
                        filter.clear();
                        let all = self.queries.all();
                        queue.queue_work(|queue| queue.background = all);
                        // The caller wants all answers.
                        queue.own.clear();
                        // All are asked again.
                        queue.start_missed.clear();
                    }
                    Some(Request::RefreshGroup(group)) => {
                        filter.clear();
                        let queries = self.queries;
                        queue.add(
                            group_queries(queries.brand, group).filter(|query| queries.wanted(query)),
                        );
                    }
                    Some(Request::Queries(queries)) => {
                        filter.clear();
                        queue.add(queries);
                    }
                    Some(Request::Synced { done, deadline, start }) => {
                        queue.add_waiter(done, deadline, start);
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
                () = tokio::time::sleep_until(signal.due.unwrap_or_else(Instant::now)), if signal.due.is_some() => {
                    self.add_signal_queries(&mut queue, signal.take_due());
                }
                () = tokio::time::sleep_until(signal.check_at.unwrap_or_else(Instant::now)), if signal.check_at.is_some() => {
                    self.check_signal(&mut queue, &mut signal);
                }
                () = tokio::time::sleep_until(answer_time.unwrap_or_else(Instant::now)), if answer_time.is_some() => {
                    queue.check_answers(Instant::now(), self.config.answer_quiet);
                }
                () = tokio::time::sleep_until(write_time.unwrap_or_else(Instant::now)), if write_time.is_some() => {
                    if let Err(end) = self.write_next(&mut stream, &mut queue, &mut line, shutdown).await {
                        break end;
                    }
                }
            }
        };
        queue.fail_all();
        drop_requests(requests);
        end
    }

    /// Writes the next line: from the caller first, then a query. A stop of
    /// the task ends the write.
    async fn write_next(
        &self,
        stream: &mut TcpStream,
        queue: &mut Queue,
        line: &mut String,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<(), End> {
        let (text, done, from_user) = if let Some(outgoing) = queue.user.pop_front() {
            (outgoing.line, outgoing.done, true)
        } else if let Some(query) = queue.background.pop_front() {
            tracing::trace!(query = %query, "telnet query");
            line.clear();
            line.push_str(&query);
            line.push('\r');
            queue.answers.written(query);
            (std::mem::take(line), None, false)
        } else {
            return Ok(());
        };
        let write =
            tokio::time::timeout(self.config.write_timeout, stream.write_all(text.as_bytes()));
        let Some(result) = until_stopped(shutdown, write).await else {
            if let Some(done) = done {
                let _ = done.send(Err(maybe_written(format!("telnet send to {}", self.host))));
            }
            return Err(End::Shutdown);
        };
        let result = result.unwrap_or_else(|_| Err(std::io::ErrorKind::TimedOut.into()));
        let now = Instant::now();
        queue.last_write = Some(now);
        if from_user {
            queue.last_user_write = Some(now);
        } else {
            queue.last_answer = now;
            queue.last_activity = now;
            // Keep the buffer for the next query.
            *line = text;
        }
        if let Some(done) = done {
            let _ = done.send(match &result {
                Ok(()) => Ok(()),
                Err(error) => Err(Error::Network {
                    request: format!("telnet send to {}", self.host),
                    source: Box::new(std::io::Error::new(error.kind(), error.to_string())),
                    // Part of the line can have been written.
                    sent: true,
                }),
            });
        }
        result.map_err(End::Failed)
    }

    /// Asks for the tuner or HD Radio values after the source changed to it.
    fn add_source_queries(&self, queue: &mut Queue, tuner: bool, hd_radio: bool) {
        if tuner {
            queue.add_own(TUNER_QUERIES);
            if self.queries.dab {
                queue.add_own(DAB_QUERIES);
            }
        }
        if hd_radio {
            queue.add_own(HD_RADIO_QUERIES);
        }
    }

    /// Adds the signal queries of a hint. Video answers that change the
    /// signal make BT.2020 not known, and that asks for it.
    fn add_signal_queries(&self, queue: &mut Queue, read: SignalRead) {
        let queries = self.queries;
        let parts = [
            (read.video, SignalPart::Video),
            (read.channels, SignalPart::Channels),
        ];
        for (_, part) in parts.into_iter().filter(|(asked, _)| *asked) {
            queue
                .add_own(signal_queries(queries.brand, part).filter(|query| queries.wanted(query)));
        }
    }

    /// The hint and check state of a new connection.
    fn signal_hint(&self, now: Instant) -> SignalHint {
        let check = self
            .config
            .signal_check
            .filter(|_| self.queries.signal_check);
        SignalHint::new(now, check, random_wait(Duration::ZERO, SIGNAL_CHECK_JITTER))
    }

    /// Sends the queries of [`SIGNAL_CHECK`] that are due, while the main zone
    /// is on.
    fn check_signal(&self, queue: &mut Queue, signal: &mut SignalHint) {
        let previous = signal.checking;
        let ask = signal.take_check(
            Instant::now(),
            random_wait(Duration::ZERO, SIGNAL_CHECK_JITTER),
        );
        let on = self
            .channels
            .read(|state| state.zone(Zone::Main).power == Some(Power::On));
        if !on {
            return;
        }
        let queries = self.queries;
        let misses = queue.check_misses;
        // A video read that is due, or waits for its write or its answers,
        // gives the values.
        let video_busy = (signal.due.is_some() && signal.read.video)
            || SIGNAL_CHECK[..SIGNAL_CHECK_VIDEO]
                .iter()
                .any(|(query, _)| queue.is_waiting(query));
        let due = |index: usize| {
            let query = SIGNAL_CHECK[index].0;
            ask[index]
                && !(video_busy && index < SIGNAL_CHECK_VIDEO)
                && misses[index] < SIGNAL_CHECK_MISSES
                && queries.wanted(query)
        };
        signal.checking = if video_busy {
            // The answers of an earlier check can still come.
            previous
        } else {
            (0..SIGNAL_CHECK_VIDEO)
                .rev()
                .find(|&index| due(index))
                .map(|index| SIGNAL_CHECK[index].1)
        };
        queue.add_own(
            (0..SIGNAL_CHECK.len())
                .filter(|&index| due(index))
                .map(|index| SIGNAL_CHECK[index].0),
        );
    }

    fn colorimetry_unknown(&self) -> bool {
        self.channels
            .read(|state| state.device().signal.needs_colorimetry_query())
    }

    /// Applies a line. Returns false if the duplicate filter dropped it.
    fn handle_line(&self, line: &str, filter: &mut DuplicateFilter) -> bool {
        let Some(message) = Message::parse(line) else {
            return true;
        };
        if !filter.accept(message.parameter) {
            tracing::trace!(
                event = message.event.code(),
                "telnet duplicate event ignored"
            );
            return false;
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
        true
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

/// Runs `work`. Requests that come in meanwhile are refused, because there is
/// no connection to send them on.
async fn refusing_requests<T>(
    requests: &mut mpsc::Receiver<Request>,
    work: impl Future<Output = T>,
) -> T {
    tokio::pin!(work);
    loop {
        tokio::select! {
            output = &mut work => return output,
            Some(request) = requests.recv() => {
                if let Request::Send(outgoing) = request {
                    outgoing.fail();
                }
            }
        }
    }
}

/// Opens a connection to the receiver.
async fn open(host: &str, config: &TelnetConfig) -> Result<TcpStream, Error> {
    let request = || format!("telnet connect {}:{}", UrlHost(host), config.port);
    match tokio::time::timeout(
        config.connect_timeout,
        TcpStream::connect((host, config.port)),
    )
    .await
    {
        Err(_) => Err(Error::Timeout {
            request: request(),
            limit: config.connect_timeout,
            sent: false,
        }),
        Ok(Err(source)) => Err(Error::Network {
            request: request(),
            source: Box::new(source),
            sent: false,
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
    use crate::avr::test_info;
    use denon_marantz_proto::Zone;
    use denon_marantz_proto::telnet::query_for_text;

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
            TelnetConfig {
                answer_quiet: Duration::ZERO,
                ..default
            },
            TelnetConfig {
                signal_wait_min: Duration::from_secs(3),
                ..default
            },
            TelnetConfig {
                signal_check: Some(Duration::ZERO),
                ..default
            },
            TelnetConfig {
                signal_check: Some(default.answer_quiet),
                ..default
            },
        ];
        for config in bad {
            assert!(
                matches!(config.check(), Err(Error::InvalidConfig(_))),
                "{config:?}"
            );
        }
        // No spacing is allowed, for tests and fast receivers.
        let none = TelnetConfig {
            command_interval: Duration::ZERO,
            ..default
        };
        assert!(none.check().is_ok());
    }

    #[test]
    fn a_hint_asks_for_the_video_signal_after_its_first_line() {
        let start = Instant::now();
        let ms = Duration::from_millis;
        let mut hint = SignalHint::default();
        hint.line("MSDOLBY ATMOS", AnswerLine::Other, start, || ms(500));
        assert_eq!(hint.due, Some(start + ms(500)));
        // Other lines and later hints of the burst do not move it.
        hint.line("PSLFE 00", AnswerLine::Other, start + ms(100), || ms(9));
        hint.line("SSINFAISSIG 25", AnswerLine::Other, start + ms(200), || {
            ms(9)
        });
        assert_eq!(hint.due, Some(start + ms(500)));
    }

    #[test]
    fn an_answer_to_a_query_of_this_connection_is_not_a_hint() {
        for answer in [AnswerLine::Answer, AnswerLine::Related] {
            let mut hint = SignalHint::default();
            hint.line("MSSTEREO", answer, Instant::now(), || {
                Duration::from_secs(1)
            });
            assert_eq!(hint.due, None, "{answer:?}");
        }
    }

    #[test]
    fn only_a_known_input_resolution_cancels_the_hint() {
        let start = Instant::now();
        let wait = || Duration::from_secs(1);
        let cancels = [
            ("SSINFSIGRES I4K24", true),
            ("SYSDVIN 4K24", true),
            // Sent by itself when the input signal is lost.
            ("SSINFSIGRES I--- ", false),
            ("SYSDVIN --- ", false),
            ("SSINFSIGRES I", false),
            ("SSINFSIGRES O4K24", false),
            ("SSINFSIGHDR I---", false),
        ];
        for (line, cancel) in cancels {
            let mut hint = SignalHint::default();
            hint.line("MSSTEREO", AnswerLine::Other, start, wait);
            hint.line(line, AnswerLine::Other, start, wait);
            assert_eq!(!hint.read.video, cancel, "{line}");
            // The channels are still asked at the same time.
            assert_eq!(hint.due, Some(start + wait()), "{line}");
            assert!(hint.read.channels, "{line}");
        }
    }

    #[test]
    fn a_hint_asks_for_the_video_signal_and_the_channels() {
        let now = Instant::now();
        let wait = || Duration::from_millis(500);
        let mut hint = SignalHint::default();
        hint.line("MSDOLBY ATMOS", AnswerLine::Other, now, wait);
        assert_eq!(
            hint.take_due(),
            SignalRead {
                video: true,
                channels: true
            }
        );
        // A known input resolution leaves only the channels.
        hint.line("SSINFAISSIG 25", AnswerLine::Other, now, wait);
        hint.line("SSINFSIGRES I4K24", AnswerLine::Other, now, wait);
        assert_eq!(
            hint.take_due(),
            SignalRead {
                video: false,
                channels: true
            }
        );
        // A read without video does not ask again.
        hint.line("SSINFSIGRES I--- ", AnswerLine::Answer, now, wait);
        assert_eq!(hint.due, None);
    }

    #[test]
    fn the_second_read_asks_only_for_the_video_signal() {
        let now = Instant::now();
        let wait = || Duration::from_millis(500);
        let mut hint = SignalHint::default();
        hint.line("MSDOLBY ATMOS", AnswerLine::Other, now, wait);
        hint.take_due();
        hint.line("SSINFSIGRES I--- ", AnswerLine::Answer, now, wait);
        assert_eq!(
            hint.take_due(),
            SignalRead {
                video: true,
                channels: false
            }
        );
    }

    #[test]
    fn a_line_that_this_connection_did_not_ask_for_skips_its_check() {
        let now = Instant::now();
        let every = Duration::from_secs(5);
        let wait = || Duration::from_millis(500);
        let mut hint = SignalHint::new(now, Some(every), Duration::ZERO);
        assert_eq!(hint.check_at, Some(now + every));
        // Sent by itself, or the answer to another client.
        hint.line("SYSDA PCM", AnswerLine::Other, now, wait);
        // Answers to this connection count as not seen.
        hint.line("OPINFINS 2211", AnswerLine::Answer, now, wait);
        hint.line("SSINFSIGRES O4K24", AnswerLine::Related, now, wait);
        let jitter = Duration::from_millis(100);
        let at = now + every;
        assert_eq!(
            hint.take_check(at, jitter),
            [true, true, true, false, true, true, true]
        );
        assert_eq!(hint.check_at, Some(at + every + jitter));
        // Only since the last check.
        assert_eq!(hint.take_check(at, jitter), [true; SIGNAL_CHECK.len()]);
        // Off.
        assert_eq!(SignalHint::new(now, None, Duration::ZERO).check_at, None);
        // The first check has its random wait too.
        let jitter = Duration::from_millis(300);
        assert_eq!(
            SignalHint::new(now, Some(every), jitter).check_at,
            Some(now + every + jitter)
        );
    }

    #[test]
    fn the_signal_check_queries_are_signal_queries() {
        for brand in [Brand::Denon, Brand::Marantz] {
            for (query, prefix) in SIGNAL_CHECK {
                assert!(
                    group_queries(brand, QueryGroup::Signal).any(|text| text == query),
                    "{query}"
                );
                // The answer starts with the command of the query.
                assert_eq!(query.split(' ').next(), prefix.split(' ').next(), "{query}");
            }
        }
        let video = signal_queries(Brand::Denon, SignalPart::Video).collect::<Vec<_>>();
        for (index, (query, _)) in SIGNAL_CHECK.iter().enumerate() {
            assert_eq!(index < SIGNAL_CHECK_VIDEO, video.contains(query), "{query}");
        }
    }

    #[test]
    fn only_a_changed_check_answer_asks_for_the_video_signal() {
        let now = Instant::now();
        let mut hint = SignalHint::new(now, Some(Duration::from_secs(30)), Duration::ZERO);
        // No check was sent.
        hint.check_line("SSINFSIGRES I4K24", AnswerLine::Answer, true, now);
        assert_eq!(hint.due, None);
        // Same values: nothing, and the HDR answer ends the check.
        hint.checking = Some("SSINFSIGHDR I");
        hint.check_line("SSINFSIGRES I4K24", AnswerLine::Answer, false, now);
        assert!(hint.checking.is_some());
        hint.check_line("SSINFSIGHDR I---", AnswerLine::Answer, false, now);
        assert_eq!(hint.checking, None);
        assert_eq!(hint.due, None);
        // An answer to another client is not the check.
        hint.checking = Some("SSINFSIGHDR I");
        hint.check_line("SSINFSIGHDR IDolby Vision", AnswerLine::Other, true, now);
        assert_eq!(hint.due, None);
        // A changed resolution asks now, before the HDR answer.
        let mut res = SignalHint::new(now, Some(Duration::from_secs(30)), Duration::ZERO);
        res.checking = Some("SSINFSIGHDR I");
        res.check_line("SSINFSIGRES I4K50", AnswerLine::Answer, true, now);
        assert_eq!(res.checking, None);
        assert_eq!(res.due, Some(now));
        // A changed value asks now.
        hint.check_line("SSINFSIGHDR IDolby Vision", AnswerLine::Answer, true, now);
        assert_eq!(hint.checking, None);
        assert_eq!(hint.due, Some(now));
        assert_eq!(
            hint.take_due(),
            SignalRead {
                video: true,
                channels: false
            }
        );
    }

    #[test]
    fn a_check_without_hdr_ends_with_the_resolution() {
        let now = Instant::now();
        let mut hint = SignalHint {
            checking: Some("SSINFSIGRES I"),
            ..SignalHint::default()
        };
        hint.check_line("SSINFSIGRES I4K24", AnswerLine::Answer, false, now);
        assert_eq!(hint.checking, None);
        // A later answer that passes the filter asks nothing.
        hint.check_line("SSINFSIGRES I4K24", AnswerLine::Answer, true, now);
        assert_eq!(hint.due, None);
    }

    #[test]
    fn a_check_answer_does_not_cancel_the_video_read_of_a_hint() {
        let now = Instant::now();
        let wait = || Duration::from_millis(500);
        let mut hint = SignalHint::default();
        hint.line("MSDOLBY ATMOS", AnswerLine::Other, now, wait);
        hint.checking = Some("SSINFSIGHDR I");
        hint.line("SSINFSIGRES I4K24", AnswerLine::Answer, now, wait);
        assert!(hint.read.video);
        // Without a check, an own answer with the values still cancels it.
        hint.checking = None;
        hint.line("SSINFSIGRES I4K24", AnswerLine::Answer, now, wait);
        assert!(!hint.read.video);
    }

    #[test]
    fn a_channel_read_keeps_the_check_going() {
        let now = Instant::now();
        let wait = || Duration::from_millis(500);
        let mut hint = SignalHint::default();
        hint.line("MSDOLBY ATMOS", AnswerLine::Other, now, wait);
        // Another client asked for the resolution.
        hint.line("SSINFSIGRES I4K24", AnswerLine::Other, now, wait);
        hint.checking = Some("SSINFSIGHDR I");
        assert_eq!(
            hint.take_due(),
            SignalRead {
                video: false,
                channels: true
            }
        );
        assert_eq!(hint.checking, Some("SSINFSIGHDR I"));
    }

    #[test]
    fn only_a_new_sample_rate_is_a_hint() {
        let now = Instant::now();
        let wait = || Duration::from_secs(1);
        let mut hint = SignalHint::default();
        hint.line("SSINFAISFSV 48K", AnswerLine::Other, now, wait);
        // The receiver repeats it every few seconds.
        hint.line("SSINFAISFSV 48K", AnswerLine::Other, now, wait);
        assert_eq!(hint.due, None);
        hint.line("SSINFAISFSV 441", AnswerLine::Other, now, wait);
        assert_eq!(hint.due, Some(now + wait()));
        // The first value is no change.
        let mut first = SignalHint::default();
        first.line("SSINFAISFSV 441", AnswerLine::Other, now, wait);
        assert_eq!(first.due, None);
    }

    #[test]
    fn an_answer_without_values_asks_once_more() {
        let now = Instant::now();
        let wait = || Duration::from_millis(500);
        let mut hint = SignalHint::default();
        hint.line("MSDOLBY ATMOS", AnswerLine::Other, now, wait);
        hint.take_due();
        hint.line("SSINFSIGRES I--- ", AnswerLine::Answer, now, wait);
        assert_eq!(hint.due, Some(now + SIGNAL_AGAIN));
        hint.take_due();
        // Only once.
        hint.line("SSINFSIGRES I--- ", AnswerLine::Answer, now, wait);
        assert_eq!(hint.due, None);
        // The same for the other resolution line.
        let mut dv = SignalHint::default();
        dv.line("MSDOLBY ATMOS", AnswerLine::Other, now, wait);
        dv.take_due();
        dv.line("SYSDVIN --- ", AnswerLine::Answer, now, wait);
        assert_eq!(dv.due, Some(now + SIGNAL_AGAIN));
    }

    #[test]
    fn a_new_sample_rate_in_an_own_answer_is_found_at_its_repeat() {
        let now = Instant::now();
        let wait = || Duration::from_millis(500);
        let mut hint = SignalHint::default();
        hint.line("SSINFAISFSV 48K", AnswerLine::Other, now, wait);
        for answer in [AnswerLine::Answer, AnswerLine::Related] {
            hint.line("SSINFAISFSV 441", answer, now, wait);
            assert_eq!(hint.due, None);
        }
        hint.line("SSINFAISFSV 441", AnswerLine::Other, now, wait);
        assert_eq!(hint.due, Some(now + wait()));
    }

    #[test]
    fn an_answer_without_values_outside_a_hint_read_asks_nothing() {
        let now = Instant::now();
        let wait = || Duration::from_millis(500);
        // An answer of the start, or of a refresh.
        let mut hint = SignalHint::default();
        hint.line("SSINFSIGRES I--- ", AnswerLine::Answer, now, wait);
        assert_eq!(hint.due, None);
        // An answer to another client.
        hint.line("MSDOLBY ATMOS", AnswerLine::Other, now, wait);
        hint.take_due();
        hint.line("SSINFSIGRES I--- ", AnswerLine::Other, now, wait);
        assert_eq!(hint.due, None);
        // Known values end the hint read.
        hint.line("SSINFSIGRES I4K24", AnswerLine::Answer, now, wait);
        hint.line("SSINFSIGRES I--- ", AnswerLine::Answer, now, wait);
        assert_eq!(hint.due, None);
    }

    #[test]
    fn the_random_wait_is_in_its_range() {
        let TelnetConfig {
            signal_wait_min: min,
            signal_wait_max: max,
            ..
        } = TelnetConfig::default();
        let waits: Vec<_> = (0..50).map(|_| random_wait(min, max)).collect();
        assert!(waits.iter().all(|wait| (min..=max).contains(wait)));
        assert!(waits.iter().any(|wait| *wait != waits[0]));
        assert_eq!(random_wait(min, min), min);
    }

    fn test_queries(brand: Brand, zones: Option<u8>, version: &str) -> Queries {
        Queries::new(QueryGroup::DEFAULT_ORDER, &test_info(version, brand, zones))
    }

    fn has(queries: &VecDeque<Cow<'static, str>>, text: &str) -> bool {
        queries.iter().any(|query| query == text)
    }

    #[test]
    fn queries_follow_the_spec_generation() {
        let all = |version| test_queries(Brand::Denon, Some(3), version).all();
        let current = all("0301");
        for text in ["PSMODE: ?", "PSFH: ?", "MNPRV?", "PSDIL ?"] {
            assert!(!has(&current, text), "{text}");
        }
        assert!(has(&current, "MV?"));
        let denon2015 = all("0300");
        assert!(has(&denon2015, "PSMODE: ?") && has(&denon2015, "PSDIL ?"));
        assert!(!has(&denon2015, "PSFH: ?") && !has(&denon2015, "MNPRV?"));
        assert!(has(&all(""), "PSAFD ?"));
    }

    #[test]
    fn power_query_depends_on_the_zones() {
        let multi = test_queries(Brand::Denon, Some(3), "").all();
        assert_eq!(multi.front().map(Cow::as_ref), Some("ZM?"));
        let single = test_queries(Brand::Marantz, Some(1), "").all();
        assert_eq!(single.front().map(Cow::as_ref), Some("PW?"));
        for (zones, power) in [(Some(0), "PW?"), (Some(4), "ZM?"), (None, "ZM?")] {
            let first = test_queries(Brand::Denon, zones, "").all().pop_front();
            assert_eq!(first.as_deref(), Some(power), "{zones:?}");
        }
        assert_eq!(
            multi.len(),
            1 + queries(Brand::Denon, QueryGroup::DEFAULT_ORDER).count()
        );
        let zone_queries = queries(Brand::Marantz, QueryGroup::DEFAULT_ORDER)
            .filter(|text| query_for_text(text).is_some_and(|query| query.zone() != Zone::Main))
            .count();
        assert_eq!(
            single.len(),
            1 + queries(Brand::Marantz, QueryGroup::DEFAULT_ORDER).count() - zone_queries
        );
    }

    #[test]
    fn queries_follow_the_zones() {
        let all = |zones| test_queries(Brand::Denon, zones, "0301").all();
        let zone2 = [
            "Z2?",
            "Z2MU?",
            "Z2SLP?",
            "Z2QUICK ?",
            "SSVCTZ2S ?",
            "Z2HDA?",
        ];
        let zone3 = ["Z3?", "Z3MU?", "Z3SLP?", "Z3QUICK ?", "SSVCTZ3S ?", "Z3CS?"];
        for (zones, has_zone2, has_zone3) in [
            (Some(1), false, false),
            (Some(2), true, false),
            (Some(3), true, true),
            (Some(4), true, true),
            (Some(0), false, false),
            (None, true, true),
        ] {
            let queries = all(zones);
            assert!(has(&queries, "MV?"), "{zones:?}");
            for text in zone2 {
                assert_eq!(has(&queries, text), has_zone2, "{zones:?} {text}");
            }
            for text in zone3 {
                assert_eq!(has(&queries, text), has_zone3, "{zones:?} {text}");
            }
        }
    }

    #[test]
    fn queries_follow_info_offers() {
        for (version, zones) in [("0301", Some(1)), ("0300", Some(2)), ("", None)] {
            for brand in [Brand::Denon, Brand::Marantz] {
                let info = test_info(version, brand, zones);
                let list = Queries::new(QueryGroup::DEFAULT_ORDER, &info);
                for text in queries(brand, QueryGroup::DEFAULT_ORDER) {
                    let query = query_for_text(text).unwrap();
                    assert_eq!(list.wanted(text), info.offers(query), "{text}");
                }
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn requests_are_refused_while_there_is_no_connection() {
        let (sender, mut requests) = mpsc::channel(4);
        let (done, refused) = oneshot::channel();
        let outgoing = Outgoing {
            line: "MVUP\r".to_owned(),
            done: Some(done),
        };
        sender.send(Request::Send(outgoing)).await.unwrap();
        sender.send(Request::Refresh).await.unwrap();
        let output = refusing_requests(&mut requests, async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            7
        })
        .await;
        assert_eq!(output, 7);
        assert!(matches!(refused.await, Ok(Err(Error::NotConnected))));
        assert!(requests.try_recv().is_err());
    }

    /// A waiter whose deadline does not come in the test.
    fn waiter(queue: &mut Queue) -> oneshot::Receiver<Result<SyncResult, Error>> {
        waiter_until(queue, Instant::now() + Duration::from_secs(3600))
    }

    fn waiter_until(
        queue: &mut Queue,
        deadline: Instant,
    ) -> oneshot::Receiver<Result<SyncResult, Error>> {
        waiter_with(queue, deadline, false)
    }

    /// As [`waiter`], for the wait of the start.
    fn start_waiter(queue: &mut Queue) -> oneshot::Receiver<Result<SyncResult, Error>> {
        waiter_with(queue, Instant::now() + Duration::from_secs(3600), true)
    }

    fn waiter_with(
        queue: &mut Queue,
        deadline: Instant,
        start: bool,
    ) -> oneshot::Receiver<Result<SyncResult, Error>> {
        let (done, waiting) = oneshot::channel();
        queue.add_waiter(done, deadline, start);
        waiting
    }

    /// The result of a wait that ended, with these queries not answered.
    fn done(not_answered: &[&'static str]) -> SyncResult {
        SyncResult::Done(not_answered.iter().copied().map(Cow::Borrowed).collect())
    }

    /// Writes the next query that waits, as the worker does.
    fn write_next(queue: &mut Queue) {
        let query = queue.background.pop_front().unwrap();
        queue.answers.written(query);
    }

    #[tokio::test(start_paused = true)]
    async fn only_the_queries_of_the_caller_are_reported() {
        let quiet = Duration::from_millis(2000);
        let mut queue = Queue::new(VecDeque::new(), Brand::Denon);
        queue.add_own(["SYSDA ?"]);
        queue.add(["MV?"]);
        write_next(&mut queue);
        write_next(&mut queue);
        let mut waiting = waiter(&mut queue);
        let mut start = start_waiter(&mut queue);
        queue.check_answers(Instant::now() + quiet, quiet);
        assert_eq!(waiting.try_recv().unwrap().unwrap(), done(&["MV?"]));
        assert_eq!(start.try_recv().unwrap().unwrap(), done(&["MV?"]));
        assert_eq!(queue.own, [] as [&str; 0]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_miss_before_the_start_waits_reports_only_the_queries_of_the_caller() {
        let quiet = Duration::from_millis(2000);
        let mut queue = Queue::new(VecDeque::new(), Brand::Denon);
        queue.add(["MV?"]);
        queue.add_own(["SYSDA ?"]);
        write_next(&mut queue);
        write_next(&mut queue);
        let later = Instant::now() + quiet;
        queue.check_answers(later, quiet);
        let mut start = start_waiter(&mut queue);
        queue.check_answers(later, quiet);
        assert_eq!(start.try_recv().unwrap().unwrap(), done(&["MV?"]));
    }

    #[tokio::test(start_paused = true)]
    async fn an_own_query_written_twice_is_not_reported() {
        let quiet = Duration::from_millis(2000);
        let mut queue = Queue::new(VecDeque::new(), Brand::Denon);
        queue.add_own(["SYSDA ?"]);
        write_next(&mut queue);
        queue.add_own(["SYSDA ?"]);
        write_next(&mut queue);
        let mut waiting = waiter(&mut queue);
        queue.check_answers(Instant::now() + quiet, quiet);
        assert_eq!(waiting.try_recv().unwrap().unwrap(), done(&[]));
        assert_eq!(queue.own, [] as [&str; 0]);
    }

    #[tokio::test(start_paused = true)]
    async fn an_own_query_that_waits_for_its_second_write_stays_own() {
        let quiet = Duration::from_millis(2000);
        let mut queue = Queue::new(VecDeque::new(), Brand::Denon);
        queue.add_own(["SYSDA ?"]);
        write_next(&mut queue);
        queue.add_own(["SYSDA ?"]);
        let mut waiting = waiter(&mut queue);
        let later = Instant::now() + quiet;
        queue.check_answers(later, quiet);
        assert_eq!(queue.own, ["SYSDA ?"]);
        write_next(&mut queue);
        queue.check_answers(later + quiet, quiet);
        assert_eq!(waiting.try_recv().unwrap().unwrap(), done(&[]));
        assert_eq!(queue.own, [] as [&str; 0]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_query_of_the_caller_stays_the_callers() {
        let quiet = Duration::from_millis(2000);
        let mut queue = Queue::new(VecDeque::new(), Brand::Denon);
        queue.add(["SYSDA ?"]);
        write_next(&mut queue);
        queue.add_own(["SYSDA ?"]);
        assert_eq!(queue.own, [] as [&str; 0]);
        queue.add_own(["SYSMI ?"]);
        // The caller asks it later.
        queue.add(["SYSMI ?"]);
        assert_eq!(queue.own, [] as [&str; 0]);
        write_next(&mut queue);
        write_next(&mut queue);
        let mut waiting = waiter(&mut queue);
        queue.check_answers(Instant::now() + quiet, quiet);
        let SyncResult::Done(mut missed) = waiting.try_recv().unwrap().unwrap() else {
            panic!("the wait did not end");
        };
        missed.sort();
        missed.dedup();
        assert_eq!(missed, ["SYSDA ?", "SYSMI ?"]);
    }

    #[tokio::test(start_paused = true)]
    async fn a_waiter_at_its_deadline_gets_no_own_queries() {
        let mut queue = Queue::new(VecDeque::new(), Brand::Denon);
        queue.add_own(["SYSDA ?"]);
        write_next(&mut queue);
        queue.add_own(["SYSMI ?"]);
        queue.add(["MV?"]);
        let now = Instant::now();
        let mut waiting = waiter_until(&mut queue, now);
        queue.end_late_waiters(now);
        let open = SyncResult::TimedOut(vec![Cow::Borrowed("MV?")]);
        assert_eq!(waiting.try_recv().unwrap().unwrap(), open);
    }

    #[tokio::test(start_paused = true)]
    async fn a_waiter_ends_when_the_answers_are_complete() {
        let ms = Duration::from_millis;
        let quiet = ms(2000);
        let mut queue = Queue::new(VecDeque::new(), Brand::Denon);
        let start = Instant::now();
        queue.answers.written(Cow::Borrowed("TR?"));
        let mut waiting = waiter(&mut queue);
        assert_eq!(queue.answer_time(quiet), Some(start + quiet));
        queue.answer_line("TR3 ON", start + ms(100));
        // More lines of the answer can follow.
        assert_eq!(queue.answer_time(quiet), Some(start + ms(400)));
        queue.answer_line("TR1 OFF", start + ms(200));
        queue.check_answers(start + ms(400), quiet);
        assert!(waiting.try_recv().is_err());
        queue.check_answers(start + ms(500), quiet);
        assert_eq!(waiting.try_recv().unwrap().unwrap(), done(&[]));
        assert_eq!(queue.answer_time(quiet), None);
    }

    #[tokio::test(start_paused = true)]
    async fn late_lines_of_an_answer_do_not_keep_a_wait_going() {
        let ms = Duration::from_millis;
        let quiet = ms(2000);
        let mut queue = Queue::new(VecDeque::new(), Brand::Denon);
        let start = Instant::now();
        queue.answers.written(Cow::Borrowed("MV?"));
        let mut waiting = waiter(&mut queue);
        queue.answer_line("MV50", start);
        // A volume ramp of another client: only the first line is close enough.
        for step in 1..20 {
            queue.answer_line("MV51", start + ms(step * 200));
        }
        assert_eq!(queue.answer_time(quiet), Some(start + ms(500)));
        queue.check_answers(start + ms(500), quiet);
        assert_eq!(waiting.try_recv().unwrap().unwrap(), done(&[]));
    }

    #[tokio::test(start_paused = true)]
    async fn a_query_without_an_answer_is_reported_after_the_quiet_time() {
        let ms = Duration::from_millis;
        let quiet = ms(2000);
        let mut queue = Queue::new(VecDeque::new(), Brand::Denon);
        let start = Instant::now();
        queue.answers.written(Cow::Borrowed("PSIMAXSWM ?"));
        queue.answers.written(Cow::Borrowed("OPSMLALL ?"));
        let mut waiting = waiter(&mut queue);
        // Lines of the list keep the wait going. Other lines do not.
        queue.answer_line("OPSMLALL MOV011Stereo", start + ms(1500));
        queue.answer_line("MVMAX 98", start + ms(3000));
        queue.check_answers(start + ms(3000), quiet);
        assert!(waiting.try_recv().is_err());
        queue.answer_line("OPSMLALL END", start + ms(3200));
        queue.check_answers(start + ms(5199), quiet);
        assert!(waiting.try_recv().is_err());
        queue.check_answers(start + ms(5200), quiet);
        let missed = waiting.try_recv().unwrap().unwrap();
        assert_eq!(missed, done(&["PSIMAXSWM ?"]));
        assert!(queue.answers.is_idle());
    }

    #[tokio::test(start_paused = true)]
    async fn a_waiter_waits_for_the_queries_that_are_not_written() {
        let quiet = Duration::from_millis(2000);
        let mut queue = Queue::new(VecDeque::from([Cow::Borrowed("SI?")]), Brand::Denon);
        let deadline = Instant::now() + Duration::from_secs(3600);
        let mut waiting = waiter_until(&mut queue, deadline);
        // Only the deadline while the query waits for its write, so the task
        // does not spin.
        assert_eq!(queue.answer_time(quiet), Some(deadline));
        let later = Instant::now() + Duration::from_secs(10);
        queue.check_answers(later, quiet);
        assert!(waiting.try_recv().is_err());
        queue.background.clear();
        queue.check_answers(later, quiet);
        assert_eq!(waiting.try_recv().unwrap().unwrap(), done(&[]));
    }

    #[tokio::test(start_paused = true)]
    async fn a_waiter_gets_the_open_queries_at_its_deadline() {
        let ms = Duration::from_millis;
        let quiet = ms(2000);
        let mut queue = Queue::new(VecDeque::new(), Brand::Denon);
        let start = Instant::now();
        queue.answers.written(Cow::Borrowed("PSIMAXSWM ?"));
        // Queries not written yet, so the wait does not end.
        queue.add(["SI?", "PSIMAXSWM ?"]);
        let mut waiting = waiter_until(&mut queue, start + ms(3000));
        queue.check_answers(start + quiet, quiet);
        assert!(waiting.try_recv().is_err());
        // A list without its end line.
        queue.answers.written(Cow::Borrowed("OPSMLALL ?"));
        queue.last_answer = start + ms(2100);
        queue.answer_line("OPSMLALL MOV011Stereo", start + ms(2200));
        assert_eq!(queue.answer_time(quiet), Some(start + ms(3000)));
        queue.check_answers(start + ms(2999), quiet);
        assert!(waiting.try_recv().is_err());
        queue.check_answers(start + ms(3000), quiet);
        let open = ["PSIMAXSWM ?", "OPSMLALL ?", "SI?"].map(Cow::Borrowed);
        assert_eq!(
            waiting.try_recv().unwrap().unwrap(),
            SyncResult::TimedOut(open.to_vec())
        );
        // The queries stay open and wait for their write.
        assert!(!queue.answers.is_idle());
        assert_eq!(queue.background.len(), 2);
        assert!(queue.waiters.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn the_wait_of_the_start_gets_the_queries_missed_before_it() {
        let quiet = Duration::from_millis(2000);
        let mut queue = Queue::new(VecDeque::new(), Brand::Denon);
        let start = Instant::now();
        queue.answers.written(Cow::Borrowed("PSIMAXSWM ?"));
        queue.check_answers(start + quiet, quiet);
        let mut other = waiter(&mut queue);
        let mut waiting = start_waiter(&mut queue);
        // Missed while the start waits: given to the waiters, not kept for a
        // later start.
        queue.answers.written(Cow::Borrowed("SSQS ?"));
        queue.check_answers(start + quiet * 2, quiet);
        assert_eq!(other.try_recv().unwrap().unwrap(), done(&["SSQS ?"]));
        assert_eq!(
            waiting.try_recv().unwrap().unwrap(),
            done(&["PSIMAXSWM ?", "SSQS ?"])
        );
        // A second wait of the start does not get them again.
        let mut again = start_waiter(&mut queue);
        queue.check_answers(start + quiet * 2, quiet);
        assert_eq!(again.try_recv().unwrap().unwrap(), done(&[]));
    }

    #[tokio::test(start_paused = true)]
    async fn open_queries_expire_without_a_waiter() {
        let quiet = Duration::from_millis(2000);
        let mut queue = Queue::new(VecDeque::new(), Brand::Denon);
        let start = Instant::now();
        queue.answers.written(Cow::Borrowed("PSIMAXSWM ?"));
        queue.check_answers(start + quiet, quiet);
        assert!(queue.answers.is_idle());
        // A caller that waits later does not get the old query.
        let mut waiting = waiter(&mut queue);
        queue.check_answers(start + quiet, quiet);
        assert_eq!(waiting.try_recv().unwrap().unwrap(), done(&[]));
    }

    #[tokio::test(start_paused = true)]
    async fn queue_orders_the_work() {
        let config = TelnetConfig {
            command_interval: Duration::from_millis(50),
            query_interval: Duration::from_millis(100),
            query_quiet: Duration::from_millis(50),
            ..TelnetConfig::default()
        };
        let ms = Duration::from_millis;
        let pause = ms(250);
        let mut queue = Queue::new(VecDeque::from([Cow::Borrowed("SI?")]), Brand::Denon);
        let start = Instant::now();
        // The first write is due at once.
        assert_eq!(queue.write_time(&config, pause), Some(start));
        // A line from the caller goes first. It waits for the command interval.
        queue.user.push_back(Outgoing {
            line: "MVUP\r".to_owned(),
            done: None,
        });
        queue.last_write = Some(start);
        assert_eq!(queue.write_time(&config, pause), Some(start + ms(50)));
        // A query waits for the longer query interval.
        queue.user.clear();
        assert_eq!(queue.write_time(&config, pause), Some(start + ms(100)));
        // After a command, queries wait for the pause.
        queue.last_user_write = Some(start);
        assert_eq!(queue.write_time(&config, pause), Some(start + ms(250)));
        // If the last write is later, the interval counts from there.
        queue.last_write = Some(start + ms(400));
        assert_eq!(queue.write_time(&config, pause), Some(start + ms(500)));
        // A query waits until the receiver is quiet, but not for ever.
        queue.last_read = Some(start + ms(480));
        assert_eq!(queue.write_time(&config, pause), Some(start + ms(530)));
        queue.last_read = Some(start + ms(1390));
        assert_eq!(queue.write_time(&config, pause), Some(start + ms(1400)));
        queue.background.clear();
        assert_eq!(queue.write_time(&config, pause), None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_query_after_a_quiet_time_waits_until_the_receiver_is_quiet() {
        let config = TelnetConfig {
            query_interval: Duration::from_millis(100),
            query_quiet: Duration::from_millis(50),
            ..TelnetConfig::default()
        };
        let ms = Duration::from_millis;
        let mut queue = Queue::new(VecDeque::new(), Brand::Denon);
        let start = Instant::now();
        queue.last_write = Some(start);
        // Long after the last write, the receiver sends events and a query
        // is added. It waits for the quiet time.
        tokio::time::advance(ms(5000)).await;
        queue.last_read = Some(start + ms(5000));
        queue.add(["SSINFSIGECI ?"]);
        assert_eq!(queue.write_time(&config, ms(0)), Some(start + ms(5050)));
        // Events that keep coming do not hold it back longer than the limit.
        queue.last_read = Some(start + ms(5990));
        assert_eq!(queue.write_time(&config, ms(0)), Some(start + ms(6000)));
        // A query added while another waits keeps the first time.
        tokio::time::advance(ms(500)).await;
        queue.add(["SSINFSIGCOS ?"]);
        assert_eq!(queue.waiting_since, Some(start + ms(5000)));
    }
}
