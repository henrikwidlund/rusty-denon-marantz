//! The client of one HEOS device.

use std::net::IpAddr;
use std::time::Duration;

use denon_marantz_heos_proto::{Command, Event, PORT, PlayState, PlayerId, Response};
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use crate::error::Error;
use crate::state::{Change, HeosStatus, PlayerState, Shared};
use crate::worker::{Connections, Request, Worker};

/// Requests that wait for the task. A caller that sends more waits.
const REQUEST_CAPACITY: usize = 64;

/// Settings of the connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeosConfig {
    /// Port of the device.
    pub port: u16,
    /// Time allowed to open a connection.
    pub connect_timeout: Duration,
    /// Time allowed to write one line.
    pub write_timeout: Duration,
    /// Time allowed for the answer to a command. Without an answer in time,
    /// the connections are opened again: a late answer would be taken for the
    /// answer to the next command.
    pub command_timeout: Duration,
    /// Time without data after which a heartbeat is sent on a connection.
    /// On the event connection, if there is still nothing after the same time
    /// again, the connections count as lost. On the command connection, the
    /// heartbeat is a command, so [`HeosConfig::command_timeout`] applies.
    pub keep_alive: Duration,
    /// Time to wait before the first try to connect again. It doubles with
    /// each failed try up to [`HeosConfig::reconnect_max`].
    pub reconnect_min: Duration,
    /// Longest time to wait between two tries to connect again.
    pub reconnect_max: Duration,
    /// Time from a now playing event until what plays is read. The device
    /// sends several events in a row, and they give one read.
    pub now_playing_delay: Duration,
}

impl Default for HeosConfig {
    fn default() -> Self {
        Self {
            port: PORT,
            connect_timeout: Duration::from_secs(5),
            write_timeout: Duration::from_secs(5),
            command_timeout: Duration::from_secs(15),
            keep_alive: Duration::from_secs(30),
            reconnect_min: Duration::from_secs(1),
            reconnect_max: Duration::from_secs(600),
            now_playing_delay: Duration::from_millis(200),
        }
    }
}

impl HeosConfig {
    fn check(&self) -> Result<(), Error> {
        let times = [
            self.connect_timeout,
            self.write_timeout,
            self.command_timeout,
            self.keep_alive,
            self.reconnect_min,
        ];
        if times.iter().any(Duration::is_zero) {
            return Err(Error::InvalidConfig(
                "connect_timeout, write_timeout, command_timeout, keep_alive and reconnect_min must be above 0",
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

/// The answer to a command that worked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    line: String,
}

impl Answer {
    pub(crate) const fn new(line: String) -> Self {
        Self { line }
    }

    /// The line as received.
    #[must_use]
    pub fn line(&self) -> &str {
        &self.line
    }

    /// The parsed line.
    #[must_use]
    pub fn response(&self) -> Response<'_> {
        // The task parsed the line before, so this works.
        #[allow(clippy::missing_panics_doc)]
        Response::parse(&self.line).expect("the line was parsed before")
    }
}

/// Client of one HEOS device, for example an AV receiver.
///
/// It keeps two connections open in a background task: one for commands and
/// one for events. The task keeps the state of the players, connects again
/// when a connection is lost, and then reads the state again. Dropping the
/// value stops the task.
#[derive(Debug)]
pub struct Heos {
    host: String,
    shared: Shared,
    requests: mpsc::Sender<Request>,
    shutdown: watch::Sender<bool>,
    task: Option<JoinHandle<()>>,
}

impl Heos {
    /// Connects with the default settings. See [`Heos::connect_with`].
    pub async fn connect(host: impl Into<String>) -> Result<Self, Error> {
        Self::connect_with(host, HeosConfig::default()).await
    }

    /// Opens the connections and reads the players and their state. Returns
    /// when the state is read. It is an error if the players cannot be read.
    ///
    /// `host` is a host name or an IP address.
    pub async fn connect_with(host: impl Into<String>, config: HeosConfig) -> Result<Self, Error> {
        config.check()?;
        let host = host.into();
        let connections = Connections::open(&host, &config).await?;
        let shared = Shared::new();
        let (shutdown, shutdown_receiver) = watch::channel(false);
        let (requests, request_receiver) = mpsc::channel(REQUEST_CAPACITY);
        let (synced, first_read) = oneshot::channel();
        let worker = Worker {
            host: host.clone(),
            config,
            shared: shared.clone(),
        };
        let task =
            tokio::spawn(worker.run(connections, synced, request_receiver, shutdown_receiver));
        // Dropping the value on an error stops the task.
        let heos = Self {
            host,
            shared,
            requests,
            shutdown,
            task: Some(task),
        };
        first_read.await.unwrap_or(Err(Error::NotConnected))?;
        Ok(heos)
    }

    /// Host name or IP address of the device.
    #[must_use]
    pub fn host(&self) -> &str {
        &self.host
    }

    /// State of the connections.
    #[must_use]
    pub fn status(&self) -> HeosStatus {
        self.shared.status()
    }

    /// Watches the state of the connections.
    #[must_use]
    pub fn watch_status(&self) -> watch::Receiver<HeosStatus> {
        self.shared.watch_status()
    }

    /// Subscribes to the changes of the state. Read the new values with
    /// [`Heos::players`] or [`Heos::player`].
    #[must_use]
    pub fn changes(&self) -> broadcast::Receiver<Change> {
        self.shared.subscribe_changes()
    }

    /// Subscribes to the events as the device sends them.
    ///
    /// [`Event::UserChanged`] has the name of the HEOS account.
    #[must_use]
    pub fn events(&self) -> broadcast::Receiver<Event<'static>> {
        self.shared.subscribe_events()
    }

    /// Watches the state of all players.
    #[must_use]
    pub fn watch_players(&self) -> watch::Receiver<Vec<PlayerState>> {
        self.shared.watch_players()
    }

    /// The state of all players.
    #[must_use]
    pub fn players(&self) -> Vec<PlayerState> {
        self.shared.read(<[PlayerState]>::to_vec)
    }

    /// The state of one player.
    #[must_use]
    pub fn player(&self, pid: PlayerId) -> Option<PlayerState> {
        self.shared
            .read(|states| states.iter().find(|state| state.pid() == pid).cloned())
    }

    /// The player with this IP address. An AV receiver has one player for
    /// all zones.
    #[must_use]
    pub fn player_by_ip(&self, ip: IpAddr) -> Option<PlayerId> {
        self.shared.read(|states| {
            states
                .iter()
                .find(|state| state.player.ip_addr() == Some(ip))
                .map(PlayerState::pid)
        })
    }

    /// Sends a command and waits for its answer.
    ///
    /// Answers to reads of the state, for example `get_now_playing_media`,
    /// also change the state.
    pub async fn send(&self, command: Command<'_>) -> Result<Answer, Error> {
        let mut line = String::new();
        command.write(&mut line).map_err(Error::InvalidCommand)?;
        let (reply, answer) = oneshot::channel();
        self.request(Request::Send {
            line,
            name: command.name(),
            reply,
        })
        .await?;
        answer.await.unwrap_or(Err(Error::NotConnected))
    }

    /// Plays, pauses or stops.
    pub async fn set_play_state(&self, pid: PlayerId, state: PlayState) -> Result<(), Error> {
        self.send(Command::SetPlayState { pid, state })
            .await
            .map(drop)
    }

    /// Plays the next item.
    pub async fn play_next(&self, pid: PlayerId) -> Result<(), Error> {
        self.send(Command::PlayNext { pid }).await.map(drop)
    }

    /// Plays the previous item.
    pub async fn play_previous(&self, pid: PlayerId) -> Result<(), Error> {
        self.send(Command::PlayPrevious { pid }).await.map(drop)
    }

    /// Reads the players and their state again. Returns when all is read.
    /// It is an error if the players cannot be read.
    pub async fn refresh(&self) -> Result<(), Error> {
        let (done, read) = oneshot::channel();
        self.request(Request::Refresh(done)).await?;
        read.await.unwrap_or(Err(Error::NotConnected))
    }

    /// Stops the task and closes the connections.
    pub async fn close(mut self) {
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }

    /// Passes a request to the task. A request is refused while there is no
    /// connection: it would be sent late, after a connect.
    async fn request(&self, request: Request) -> Result<(), Error> {
        if self.status() != HeosStatus::Connected {
            return Err(Error::NotConnected);
        }
        self.requests
            .send(request)
            .await
            .map_err(|_| Error::NotConnected)
    }
}

impl Drop for Heos {
    fn drop(&mut self) {
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_are_valid() {
        assert!(HeosConfig::default().check().is_ok());
    }

    #[test]
    fn settings_are_checked() {
        let default = HeosConfig::default();
        let bad = [
            HeosConfig {
                connect_timeout: Duration::ZERO,
                ..default
            },
            HeosConfig {
                write_timeout: Duration::ZERO,
                ..default
            },
            HeosConfig {
                command_timeout: Duration::ZERO,
                ..default
            },
            HeosConfig {
                keep_alive: Duration::ZERO,
                ..default
            },
            HeosConfig {
                reconnect_min: Duration::ZERO,
                ..default
            },
            HeosConfig {
                reconnect_min: Duration::from_secs(700),
                ..default
            },
        ];
        for config in bad {
            assert!(
                matches!(config.check(), Err(Error::InvalidConfig(_))),
                "{config:?}"
            );
        }
        // No delay is allowed.
        let none = HeosConfig {
            now_playing_delay: Duration::ZERO,
            ..default
        };
        assert!(none.check().is_ok());
    }
}
