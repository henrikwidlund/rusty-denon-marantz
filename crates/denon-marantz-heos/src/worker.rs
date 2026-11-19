//! The background task. It owns two connections: one for commands and one
//! for events.
//!
//! Commands go one at a time. The task waits for the answer, also through
//! "command under process" up to a limit, before it writes the next one.
//! Answers to reads of the state, and to `set_play_mode`, also change the
//! state. When a connection is idle, the task sends a heartbeat. When a connection is lost, both are opened again, and
//! the players and their state are read again.

use std::collections::VecDeque;
use std::future::Future;
use std::time::Duration;

use denon_marantz_heos_proto::{
    Command, Event, Failure, LineDecoder, Player, PlayerId, RepeatMode, Response, Status,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::time::Instant;

use crate::client::{Answer, HeosConfig};
use crate::error::Error;
use crate::host::UrlHost;
use crate::state::{Change, HeosStatus, Shared};

/// Bytes read at once.
const READ_BUFFER: usize = 4096;

/// Highest position or length that is used (`i32::MAX`). After `AirPlay`
/// starts, the device can send a negative position as a 32-bit number without
/// a sign, for example 4294937296 for -30000. A length like that was not seen,
/// but is checked the same way.
const MAX_PROGRESS_MS: u64 = 2_147_483_647;

const GET_PLAYERS: &str = Command::GetPlayers.name();
const GET_PLAY_STATE: &str = Command::GetPlayState { pid: PlayerId(0) }.name();
const GET_NOW_PLAYING: &str = Command::GetNowPlayingMedia { pid: PlayerId(0) }.name();
const GET_PLAY_MODE: &str = Command::GetPlayMode { pid: PlayerId(0) }.name();
const SET_PLAY_MODE: &str = Command::SetPlayMode {
    pid: PlayerId(0),
    repeat: RepeatMode::Off,
    shuffle: false,
}
.name();

pub(crate) type AnswerSender = oneshot::Sender<Result<Answer, Error>>;
pub(crate) type DoneSender = oneshot::Sender<Result<(), Error>>;

/// What the task is asked to do.
pub(crate) enum Request {
    /// Send a command line and wait for its answer.
    Send {
        line: String,
        name: &'static str,
        reply: AnswerSender,
    },
    /// Read the players and their state again. `done` is told when all is
    /// read.
    Refresh(DoneSender),
}

/// Work that waits for the command connection.
enum Job {
    Command {
        line: String,
        name: &'static str,
        reply: Option<AnswerSender>,
    },
    /// Told when the jobs before it are done.
    Synced(DoneSender),
}

impl Job {
    /// A command of the library itself.
    fn internal(command: Command<'_>) -> Self {
        let mut line = String::new();
        // The commands of the library are valid.
        let _ = command.write(&mut line);
        Self::Command {
            line,
            name: command.name(),
            reply: None,
        }
    }

    /// Tells the caller that the session ended. A command was not sent. A
    /// waiter for [`Job::Synced`] gets `error`, which says why the session
    /// ended.
    fn fail(self, error: impl FnOnce() -> Error) {
        match self {
            Self::Command {
                reply: Some(reply), ..
            } => {
                let _ = reply.send(Err(Error::NotConnected));
            }
            Self::Synced(done) => {
                let _ = done.send(Err(error()));
            }
            Self::Command { reply: None, .. } => {}
        }
    }
}

/// Why the last read of the players did not work.
enum PlayersError {
    Failed(Failure<'static>),
    Invalid(denon_marantz_heos_proto::Error),
}

impl PlayersError {
    fn to_error(&self) -> Error {
        match self {
            Self::Failed(failure) => Error::Failed {
                command: GET_PLAYERS,
                failure: failure.clone(),
            },
            Self::Invalid(source) => Error::Protocol {
                command: GET_PLAYERS,
                source: source.clone(),
            },
        }
    }
}

/// A command that was written and waits for its answer.
struct Pending {
    name: &'static str,
    reply: Option<AnswerSender>,
    sent: Instant,
    deadline: Instant,
}

impl Pending {
    /// The time that was allowed when the deadline passed.
    fn limit(&self) -> Duration {
        self.deadline - self.sent
    }
}

/// The two connections to the device.
pub(crate) struct Connections {
    commands: TcpStream,
    events: TcpStream,
}

impl Connections {
    pub(crate) async fn open(host: &str, config: &HeosConfig) -> Result<Self, Error> {
        let (commands, events) = tokio::try_join!(open(host, config), open(host, config))?;
        Ok(Self { commands, events })
    }
}

/// Why a session ended.
enum End {
    Shutdown,
    Closed,
    Failed(std::io::Error),
    Silent,
    Timeout {
        command: &'static str,
        limit: Duration,
    },
}

impl std::fmt::Display for End {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Shutdown => f.write_str("stopped"),
            Self::Closed => f.write_str("closed by the device"),
            Self::Failed(error) => write!(f, "read or write failed: {error}"),
            Self::Silent => f.write_str("no answer to the heartbeat on the event connection"),
            Self::Timeout { command, .. } => write!(f, "no answer to {command}"),
        }
    }
}

/// Sets the status to disconnected when the task ends, also if it is aborted.
struct StatusGuard(Shared);

impl Drop for StatusGuard {
    fn drop(&mut self) {
        self.0.set_status(HeosStatus::Disconnected);
    }
}

pub(crate) struct Worker {
    pub(crate) host: String,
    pub(crate) config: HeosConfig,
    pub(crate) shared: Shared,
}

impl Worker {
    /// Runs sessions until the task is stopped. `synced` is told when the
    /// first session has read the players and their state.
    pub(crate) async fn run(
        self,
        first: Connections,
        synced: DoneSender,
        mut requests: mpsc::Receiver<Request>,
        mut shutdown: watch::Receiver<bool>,
    ) {
        let _guard = StatusGuard(self.shared.clone());
        let mut connections = Some(first);
        let mut synced = Some(synced);
        let mut reconnected = false;
        loop {
            let connections = if let Some(connections) = connections.take() {
                connections
            } else {
                let Some(connections) = self.reconnect(&mut shutdown, &mut requests).await else {
                    return;
                };
                // Requests that came while there was no connection are late.
                drop_requests(&mut requests);
                connections
            };
            self.shared.set_status(HeosStatus::Connected);
            tracing::debug!(host = %self.host, "HEOS connected");
            let end = Session::new(&self, synced.take(), reconnected)
                .run(connections, &mut requests, &mut shutdown)
                .await;
            self.shared.set_status(HeosStatus::Disconnected);
            if matches!(end, End::Shutdown) {
                return;
            }
            reconnected = true;
            tracing::debug!(host = %self.host, reason = %end, "HEOS connection lost");
        }
    }

    /// Tries to connect until it works. `None` if the task is stopped.
    ///
    /// It waits before each try, longer after each failed one. A request
    /// that comes in the meantime is refused at once.
    async fn reconnect(
        &self,
        shutdown: &mut watch::Receiver<bool>,
        requests: &mut mpsc::Receiver<Request>,
    ) -> Option<Connections> {
        let mut wait = self.config.reconnect_min;
        loop {
            until_stopped(
                shutdown,
                refusing_requests(requests, tokio::time::sleep(wait)),
            )
            .await?;
            let opened = until_stopped(
                shutdown,
                refusing_requests(requests, Connections::open(&self.host, &self.config)),
            )
            .await?;
            match opened {
                Ok(connections) => return Some(connections),
                Err(error) => tracing::debug!(host = %self.host, %error, "HEOS reconnect failed"),
            }
            wait = (wait * 2).min(self.config.reconnect_max);
        }
    }
}

/// One session: from a connect until a connection is lost.
struct Session<'a> {
    worker: &'a Worker,
    queue: VecDeque<Job>,
    pending: Option<Pending>,
    /// Players whose now playing is read at this time. A burst of events
    /// gives one read.
    now_playing_due: Vec<(PlayerId, Instant)>,
    /// Last read or write on the command connection.
    command_active: Instant,
    /// Last read on the event connection.
    event_read: Instant,
    /// A heartbeat was written on the event connection and nothing came yet.
    event_asked: bool,
    /// The last read of the players did not work. A waiter for
    /// [`Job::Synced`] gets this error.
    players_error: Option<PlayersError>,
    /// The queues may have changed while there was no connection. Subscribers
    /// are told after the players are read.
    queues_unknown: bool,
}

/// What a timer asks the session to do.
enum Due {
    Nothing,
    EventHeartbeat,
}

impl<'a> Session<'a> {
    fn new(worker: &'a Worker, synced: Option<DoneSender>, reconnected: bool) -> Self {
        let now = Instant::now();
        let mut queue = VecDeque::from([Job::internal(Command::GetPlayers)]);
        queue.extend(synced.map(Job::Synced));
        Self {
            worker,
            queue,
            pending: None,
            now_playing_due: Vec::new(),
            command_active: now,
            event_read: now,
            event_asked: false,
            players_error: None,
            queues_unknown: reconnected,
        }
    }

    const fn config(&self) -> &HeosConfig {
        &self.worker.config
    }

    const fn shared(&self) -> &Shared {
        &self.worker.shared
    }

    async fn run(
        mut self,
        mut connections: Connections,
        requests: &mut mpsc::Receiver<Request>,
        shutdown: &mut watch::Receiver<bool>,
    ) -> End {
        let end = self.serve(&mut connections, requests, shutdown).await;
        let error = || match end {
            End::Timeout { command, limit } => Error::Timeout {
                request: command.to_owned(),
                limit,
            },
            _ => Error::NotConnected,
        };
        if let Some(Pending {
            reply: Some(reply), ..
        }) = self.pending.take()
        {
            let _ = reply.send(Err(error()));
        }
        for job in self.queue.drain(..) {
            job.fail(error);
        }
        drop_requests(requests);
        end
    }

    async fn serve(
        &mut self,
        connections: &mut Connections,
        requests: &mut mpsc::Receiver<Request>,
        shutdown: &mut watch::Receiver<bool>,
    ) -> End {
        let mut register = String::new();
        let _ = Command::RegisterForChangeEvents { enable: true }.write(&mut register);
        if let Err(end) = self
            .write(&mut connections.events, &register, shutdown)
            .await
        {
            return end;
        }
        let mut command_decoder = LineDecoder::new();
        let mut event_decoder = LineDecoder::new();
        let mut command_buffer = [0_u8; READ_BUFFER];
        let mut event_buffer = [0_u8; READ_BUFFER];
        loop {
            if let Err(end) = self.write_next(&mut connections.commands, shutdown).await {
                return end;
            }
            let wake = self.next_wake();
            tokio::select! {
                biased;
                _ = shutdown.changed() => return End::Shutdown,
                read = connections.commands.read(&mut command_buffer) => match read {
                    Ok(0) => return End::Closed,
                    Ok(count) => {
                        self.command_active = Instant::now();
                        command_decoder.decode(&command_buffer[..count], |line| self.on_answer(line));
                    }
                    Err(error) => return End::Failed(error),
                },
                read = connections.events.read(&mut event_buffer) => match read {
                    Ok(0) => return End::Closed,
                    Ok(count) => {
                        self.event_read = Instant::now();
                        self.event_asked = false;
                        event_decoder.decode(&event_buffer[..count], |line| self.on_event(line));
                    }
                    Err(error) => return End::Failed(error),
                },
                request = requests.recv() => match request {
                    Some(Request::Send { line, name, reply }) => {
                        self.queue.push_back(Job::Command { line, name, reply: Some(reply) });
                    }
                    Some(Request::Refresh(done)) => {
                        self.queue_once(Job::internal(Command::GetPlayers));
                        self.queue.push_back(Job::Synced(done));
                    }
                    None => return End::Shutdown,
                },
                () = tokio::time::sleep_until(wake) => match self.on_timer(Instant::now()) {
                    Ok(Due::Nothing) => {}
                    Ok(Due::EventHeartbeat) => {
                        let mut line = String::new();
                        let _ = Command::HeartBeat.write(&mut line);
                        tracing::trace!(host = %self.worker.host, "HEOS event connection idle, sending a heartbeat");
                        if let Err(end) = self.write(&mut connections.events, &line, shutdown).await {
                            return end;
                        }
                    }
                    Err(end) => return end,
                },
            }
        }
    }

    /// Writes the next command if none waits for its answer.
    async fn write_next(
        &mut self,
        stream: &mut TcpStream,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<(), End> {
        while self.pending.is_none() {
            let Some(job) = self.queue.pop_front() else {
                return Ok(());
            };
            let (line, name, reply) = match job {
                Job::Synced(done) => {
                    let _ = done.send(
                        self.players_error
                            .as_ref()
                            .map_or(Ok(()), |error| Err(error.to_error())),
                    );
                    continue;
                }
                // The caller stopped waiting: sending it now would be too late.
                Job::Command {
                    name,
                    reply: Some(reply),
                    ..
                } if reply.is_closed() => {
                    tracing::trace!(command = name, "HEOS command dropped, nobody waits");
                    continue;
                }
                Job::Command { line, name, reply } => (line, name, reply),
            };
            tracing::trace!(command = name, "HEOS command");
            if let Err(end) = self.write(stream, &line, shutdown).await {
                if let Some(reply) = reply {
                    let _ = reply.send(Err(Error::NotConnected));
                }
                return Err(end);
            }
            let now = Instant::now();
            self.command_active = now;
            self.pending = Some(Pending {
                name,
                reply,
                sent: now,
                deadline: now + self.config().command_timeout,
            });
        }
        Ok(())
    }

    /// Writes a line. A stop of the task ends the write.
    async fn write(
        &self,
        stream: &mut TcpStream,
        line: &str,
        shutdown: &mut watch::Receiver<bool>,
    ) -> Result<(), End> {
        let write = tokio::time::timeout(
            self.config().write_timeout,
            stream.write_all(line.as_bytes()),
        );
        match until_stopped(shutdown, write).await {
            None => Err(End::Shutdown),
            Some(Ok(Ok(()))) => Ok(()),
            Some(Ok(Err(error))) => Err(End::Failed(error)),
            Some(Err(_)) => Err(End::Failed(std::io::ErrorKind::TimedOut.into())),
        }
    }

    /// The next time that a timer is due.
    fn next_wake(&self) -> Instant {
        let config = self.config();
        let mut wake = self.event_read + config.keep_alive;
        if let Some(pending) = &self.pending {
            wake = wake.min(pending.deadline);
        } else if self.queue.is_empty() {
            wake = wake.min(self.command_active + config.keep_alive);
        }
        for &(_, due) in &self.now_playing_due {
            wake = wake.min(due);
        }
        wake
    }

    fn on_timer(&mut self, now: Instant) -> Result<Due, End> {
        let config = *self.config();
        if let Some(pending) = self.pending.take_if(|pending| pending.deadline <= now) {
            let limit = pending.limit();
            if let Some(reply) = pending.reply {
                let _ = reply.send(Err(Error::Timeout {
                    request: pending.name.to_owned(),
                    limit,
                }));
            }
            // A late answer would be taken for the answer to the next command.
            return Err(End::Timeout {
                command: pending.name,
                limit,
            });
        }
        let mut index = 0;
        while index < self.now_playing_due.len() {
            let (pid, due) = self.now_playing_due[index];
            if due <= now {
                self.now_playing_due.swap_remove(index);
                self.queue_once(Job::internal(Command::GetNowPlayingMedia { pid }));
            } else {
                index += 1;
            }
        }
        if self.pending.is_none()
            && self.queue.is_empty()
            && self.command_active + config.keep_alive <= now
        {
            tracing::trace!(host = %self.worker.host, "HEOS command connection idle, sending a heartbeat");
            self.queue.push_back(Job::internal(Command::HeartBeat));
        }
        if self.event_read + config.keep_alive <= now {
            if self.event_asked {
                return Err(End::Silent);
            }
            self.event_asked = true;
            // Wait the same time again for data.
            self.event_read = now;
            return Ok(Due::EventHeartbeat);
        }
        Ok(Due::Nothing)
    }

    /// Adds a command of the library, if the same one does not wait already.
    fn queue_once(&mut self, job: Job) {
        let Job::Command { line, .. } = &job else {
            return self.queue.push_back(job);
        };
        let waits = self.queue.iter().any(|queued| {
            matches!(queued, Job::Command { line: queued, reply: None, .. } if queued == line)
        });
        if !waits {
            self.queue.push_back(job);
        }
    }

    /// A line from the command connection.
    fn on_answer(&mut self, line: &str) {
        log_line(line);
        let Ok(response) = Response::parse(line) else {
            tracing::debug!(host = %self.worker.host, "HEOS line that is not valid");
            return;
        };
        if response.is_event() {
            return;
        }
        let Some(name) = self.pending.as_ref().map(|pending| pending.name) else {
            tracing::debug!(
                command = response.command(),
                "HEOS answer without a command"
            );
            return;
        };
        if response.command() != name {
            tracing::debug!(
                command = response.command(),
                "HEOS answer to another command"
            );
            return;
        }
        let result = match response.status() {
            Status::UnderProcess => {
                self.extend_deadline();
                return;
            }
            Status::Success => {
                self.apply(&response);
                Ok(())
            }
            Status::Fail(failure) => {
                tracing::debug!(command = name, code = ?failure.code, "HEOS command failed");
                if name == GET_PLAYERS {
                    self.players_failed(PlayersError::Failed(failure.clone().into_owned()));
                }
                Err(failure)
            }
        };
        if let Some(reply) = self.pending.take().and_then(|pending| pending.reply) {
            let _ = reply.send(match result {
                Ok(()) => Ok(Answer::new(line.to_owned())),
                Err(failure) => Err(Error::Failed {
                    command: name,
                    failure: failure.into_owned(),
                }),
            });
        }
    }

    /// The device still works on the pending command: allow the command
    /// timeout again, up to the slow command limit after it was sent.
    fn extend_deadline(&mut self) {
        let config = *self.config();
        if let Some(pending) = &mut self.pending {
            let limit = config.slow_command_limit.max(config.command_timeout);
            pending.deadline = (Instant::now() + config.command_timeout).min(pending.sent + limit);
        }
    }

    /// Changes the state with the answer to a read, or to a command whose
    /// answer has the new values.
    fn apply(&mut self, response: &Response<'_>) {
        let message = response.message();
        let pid = || message.get("pid")?.parse::<PlayerId>().ok();
        match response.command() {
            GET_PLAYERS => match response.payload::<Vec<Player<'_>>>() {
                Ok(players) => {
                    // Read the state of each player before other work.
                    for player in players.iter().rev() {
                        let pid = player.pid;
                        self.queue
                            .push_front(Job::internal(Command::GetPlayMode { pid }));
                        self.queue
                            .push_front(Job::internal(Command::GetNowPlayingMedia { pid }));
                        self.queue
                            .push_front(Job::internal(Command::GetPlayState { pid }));
                    }
                    self.shared().set_players(players);
                    if std::mem::take(&mut self.queues_unknown) {
                        self.shared().read(|states| {
                            for state in states {
                                self.shared().publish(Change::Queue(state.pid()));
                            }
                        });
                    }
                    self.players_error = None;
                }
                Err(error) => {
                    tracing::debug!(%error, "HEOS players not valid");
                    self.players_failed(PlayersError::Invalid(error));
                }
            },
            GET_PLAY_STATE => match (pid(), response.play_state()) {
                (Some(pid), Ok(state)) => self.shared().set_play_state(pid, state),
                (_, Err(error)) => tracing::debug!(%error, "HEOS play state not valid"),
                (None, Ok(_)) => {}
            },
            GET_NOW_PLAYING => match (pid(), response.now_playing()) {
                // An item without any text means that the title is not known
                // yet, for example when AirPlay starts.
                (Some(pid), Ok(media)) => self
                    .shared()
                    .set_now_playing(pid, media.filter(|media| !media.is_empty())),
                (_, Err(error)) => tracing::debug!(%error, "HEOS now playing not valid"),
                (None, Ok(_)) => {}
            },
            GET_PLAY_MODE | SET_PLAY_MODE => match (pid(), response.play_mode()) {
                (Some(pid), Ok(mode)) => {
                    self.shared()
                        .set_play_mode(pid, Some(mode.repeat), Some(mode.shuffle));
                }
                (_, Err(error)) => tracing::debug!(%error, "HEOS play mode not valid"),
                (None, Ok(_)) => {}
            },
            _ => {}
        }
    }

    /// Notes that the players could not be read, and tells the subscribers.
    fn players_failed(&mut self, error: PlayersError) {
        self.players_error = Some(error);
        self.shared().publish(Change::PlayersNotRead);
    }

    /// A line from the event connection.
    fn on_event(&mut self, line: &str) {
        log_line(line);
        let Ok(response) = Response::parse(line) else {
            tracing::debug!(host = %self.worker.host, "HEOS line that is not valid");
            return;
        };
        // The answers to the register command and to heartbeats are not events.
        let event = match response.event() {
            Ok(Some(event)) => event,
            Ok(None) => {
                if let Status::Fail(failure) = response.status() {
                    tracing::debug!(command = response.command(), code = ?failure.code, "HEOS command on the event connection failed");
                }
                return;
            }
            Err(error) => {
                tracing::debug!(event = response.command(), %error, "HEOS event not valid");
                return;
            }
        };
        let shared = self.shared();
        match &event {
            Event::PlayerStateChanged { pid, state } => shared.set_play_state(*pid, *state),
            Event::NowPlayingChanged { pid } => {
                if !self.now_playing_due.iter().any(|(due, _)| due == pid) {
                    let due = Instant::now() + self.config().now_playing_delay;
                    self.now_playing_due.push((*pid, due));
                }
            }
            Event::NowPlayingProgress { pid, progress } => {
                if progress.position_ms > MAX_PROGRESS_MS || progress.duration_ms > MAX_PROGRESS_MS
                {
                    tracing::debug!(
                        position_ms = progress.position_ms,
                        duration_ms = progress.duration_ms,
                        "HEOS progress not valid, skipped"
                    );
                } else {
                    shared.set_progress(*pid, *progress);
                }
            }
            Event::PlaybackError { pid, error } => shared.publish(Change::PlaybackError {
                pid: *pid,
                error: error.as_ref().into(),
            }),
            Event::RepeatModeChanged { pid, repeat } => {
                shared.set_play_mode(*pid, Some(*repeat), None);
            }
            Event::ShuffleModeChanged { pid, shuffle } => {
                shared.set_play_mode(*pid, None, Some(*shuffle));
            }
            Event::QueueChanged { pid } => shared.publish(Change::Queue(*pid)),
            Event::PlayersChanged => self.queue_once(Job::internal(Command::GetPlayers)),
            _ => {}
        }
        self.shared().send_event(event);
    }
}

/// Logs a line. Without `sensitive-logs`, only the command name: a line can
/// have personal data, for example a title or the name of the HEOS account.
fn log_line(line: &str) {
    #[cfg(feature = "sensitive-logs")]
    tracing::trace!(line, "HEOS line");
    #[cfg(not(feature = "sensitive-logs"))]
    if tracing::enabled!(tracing::Level::TRACE)
        && let Ok(response) = Response::parse(line)
    {
        tracing::trace!(command = response.command(), "HEOS line");
    }
}

/// Tells the callers of waiting requests that nothing was sent.
fn drop_requests(requests: &mut mpsc::Receiver<Request>) {
    while let Ok(request) = requests.try_recv() {
        refuse(request);
    }
}

fn refuse(request: Request) {
    match request {
        Request::Send { reply, .. } => {
            let _ = reply.send(Err(Error::NotConnected));
        }
        Request::Refresh(done) => {
            let _ = done.send(Err(Error::NotConnected));
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
            Some(request) = requests.recv() => refuse(request),
        }
    }
}

/// Opens one connection to the device.
async fn open(host: &str, config: &HeosConfig) -> Result<TcpStream, Error> {
    let request = || format!("connect {}:{}", UrlHost(host), config.port);
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
            source,
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
