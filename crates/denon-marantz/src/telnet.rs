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
use denon_marantz_proto::{Brand, HD_RADIO_QUERIES, TUNER_QUERIES};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Mutex, broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

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
}

impl Queries {
    fn all(self) -> VecDeque<Cow<'static, str>> {
        let power = if self.single_zone {
            DEVICE_POWER_QUERY
        } else {
            MAIN_ZONE_POWER_QUERY
        };
        std::iter::once(power)
            .chain(queries(self.brand, self.groups))
            .map(Cow::Borrowed)
            .collect()
    }
}

impl Telnet {
    pub(crate) fn new(
        config: TelnetConfig,
        background_pause: Duration,
        brand: Brand,
        single_zone: bool,
    ) -> Self {
        Self {
            config,
            background_pause,
            queries: Queries {
                brand,
                single_zone,
                groups: config.query_groups,
            },
            events: broadcast::Sender::new(EVENT_CAPACITY),
            status: watch::Sender::new(TelnetStatus::Disconnected),
            forget: Arc::new(AtomicBool::new(false)),
            task: Mutex::new(None),
        }
    }

    /// The connection asks for this group after a connect.
    pub(crate) fn fetches(&self, group: QueryGroup) -> bool {
        self.config.query_groups.contains(&group)
    }

    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<TelnetEvent> {
        self.events.subscribe()
    }

    pub(crate) fn watch_status(&self) -> watch::Receiver<TelnetStatus> {
        self.status.subscribe()
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
            .collect()
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
        }
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
    fn answer_line(&mut self, line: &str, now: Instant) {
        match self.answers.line(line) {
            AnswerLine::Answer => {
                self.last_answer = now;
                self.last_activity = now;
            }
            AnswerLine::Related if now < self.last_answer + ANSWER_SETTLE => {
                self.last_activity = now;
            }
            _ => {}
        }
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
            let missed: Vec<_> = self.answers.take_open().collect();
            tracing::debug!(queries = ?missed, "telnet queries not answered");
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
    fn end_late_waiters(&mut self, now: Instant) {
        for waiter in self.waiters.extract_if(.., |waiter| waiter.deadline <= now) {
            let mut open = waiter.missed;
            for query in self.answers.open().chain(&self.background) {
                if !open.contains(query) {
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
                            // Before the filter: a repeated value still answers.
                            queue.answer_line(text, idle_since);
                            self.handle_line(text, &mut filter);
                        });
                        if tuner {
                            queue.add(TUNER_QUERIES);
                        }
                        if hd_radio {
                            queue.add(HD_RADIO_QUERIES);
                        }
                        // A change made BT.2020 not known: ask once more. The
                        // answer can repeat the last value, so the filter must
                        // let it through. This asks only when it becomes not
                        // known: if a query from HTTP failed before, it stays
                        // off until the next change or `refresh_signal`.
                        if !unknown_before && self.colorimetry_unknown() {
                            filter.forget_colorimetry();
                            queue.add(signal_queries(self.queries.brand, SignalPart::Colorimetry));
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
                        // All are asked again.
                        queue.start_missed.clear();
                    }
                    Some(Request::RefreshGroup(group)) => {
                        filter.clear();
                        queue.add(group_queries(self.queries.brand, group));
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

    fn colorimetry_unknown(&self) -> bool {
        self.channels
            .read(|state| state.device().signal.needs_colorimetry_query())
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
    fn power_query_depends_on_the_zones() {
        let multi = Queries {
            brand: Brand::Denon,
            single_zone: false,
            groups: QueryGroup::DEFAULT_ORDER,
        }
        .all();
        assert_eq!(multi.front().map(Cow::as_ref), Some("ZM?"));
        let single = Queries {
            brand: Brand::Marantz,
            single_zone: true,
            groups: QueryGroup::DEFAULT_ORDER,
        }
        .all();
        assert_eq!(single.front().map(Cow::as_ref), Some("PW?"));
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
