//! The client of one HEOS device.

use std::net::IpAddr;
use std::time::Duration;

use denon_marantz_heos_proto::Error as ProtoError;
use denon_marantz_heos_proto::{
    BrowseItem, Command, Event, PORT, PlayMode, PlayState, PlayerId, QueueId, QueueItem,
    QuickSelect, Response,
};
use serde::Deserialize;
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
    /// answer to the next command. See also
    /// [`HeosConfig::slow_command_limit`].
    pub command_timeout: Duration,
    /// Longest time for the answer to a command that the device is still
    /// working on. Each "command under process" answer gives
    /// [`HeosConfig::command_timeout`] again, up to this time after the
    /// command was sent. Browsing HEOS Favorites sends such an answer. A
    /// limit below [`HeosConfig::command_timeout`] gives no extra time.
    pub slow_command_limit: Duration,
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
            slow_command_limit: Duration::from_secs(60),
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

/// One part of a list that the device sends in parts.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Page<T> {
    /// The items of this part.
    pub items: Vec<T>,
    /// Number of items in the whole list, if the device sent it. For HEOS
    /// Favorites, 0 can mean that the number is not known.
    pub count: Option<u32>,
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
    /// `host` is a host name or an IP address. An IPv6 address can have
    /// brackets. Link-local IPv6 addresses with a zone, for example
    /// `fe80::1%en0`, are not supported.
    pub async fn connect_with(host: impl Into<String>, config: HeosConfig) -> Result<Self, Error> {
        config.check()?;
        let host = crate::host::into_bare(host.into());
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

    /// The player with this IP address. The AVC-A10H has one player for all
    /// its zones.
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
    /// If the future is dropped before the command is sent, it is not sent.
    ///
    /// Answers to reads of the state, for example `get_now_playing_media`,
    /// and to `set_play_mode` also change the state.
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

    /// Sets the repeat and shuffle modes. The state changes when the answer
    /// comes.
    pub async fn set_play_mode(&self, pid: PlayerId, mode: PlayMode) -> Result<(), Error> {
        self.send(Command::SetPlayMode {
            pid,
            repeat: mode.repeat,
            shuffle: mode.shuffle,
        })
        .await
        .map(drop)
    }

    /// Reads a part of the queue. `range` is the first and last index, from
    /// 0. Without it, the device sends the first items, at most 100.
    ///
    /// The queue can change between two reads. [`Change::Queue`] tells when,
    /// also after a reconnect.
    pub async fn queue(
        &self,
        pid: PlayerId,
        range: Option<(u32, u32)>,
    ) -> Result<Page<QueueItem<'static>>, Error> {
        self.read(Command::GetQueue { pid, range }, |response| {
            page(response, QueueItem::into_owned)
        })
        .await
    }

    /// Plays an item of the queue.
    pub async fn play_queue_item(&self, pid: PlayerId, qid: QueueId) -> Result<(), Error> {
        self.send(Command::PlayQueue { pid, qid }).await.map(drop)
    }

    /// Removes items from the queue. `qids` must not be empty.
    pub async fn remove_from_queue(&self, pid: PlayerId, qids: &[QueueId]) -> Result<(), Error> {
        self.send(Command::RemoveFromQueue { pid, qids })
            .await
            .map(drop)
    }

    /// Removes all items from the queue.
    pub async fn clear_queue(&self, pid: PlayerId) -> Result<(), Error> {
        self.send(Command::ClearQueue { pid }).await.map(drop)
    }

    /// Reads a part of the HEOS Favorites. `range` is the first and last
    /// index, from 0. Without it, the device sends the first items, at most
    /// 50 or 100.
    ///
    /// The device sends no event when the Favorites change, so read them
    /// again when they are needed.
    pub async fn favorites(
        &self,
        range: Option<(u32, u32)>,
    ) -> Result<Page<BrowseItem<'static>>, Error> {
        self.read(Command::GetFavorites { range }, |response| {
            page(response, BrowseItem::into_owned)
        })
        .await
    }

    /// Plays a HEOS Favorite. `preset` is its position, from 1.
    pub async fn play_preset(&self, pid: PlayerId, preset: u32) -> Result<(), Error> {
        self.send(Command::PlayPreset { pid, preset })
            .await
            .map(drop)
    }

    /// Reads the names of the quick select slots.
    pub async fn quick_selects(&self, pid: PlayerId) -> Result<Vec<QuickSelect<'static>>, Error> {
        self.read(Command::GetQuickSelects { pid }, |response| {
            list(response, QuickSelect::into_owned)
        })
        .await
    }

    /// Plays a quick select slot, from 1 to 6.
    pub async fn play_quick_select(&self, pid: PlayerId, id: u8) -> Result<(), Error> {
        self.send(Command::PlayQuickSelect { pid, id })
            .await
            .map(drop)
    }

    /// Plays a stream from an absolute URL.
    pub async fn play_stream(&self, pid: PlayerId, url: &str) -> Result<(), Error> {
        self.send(Command::PlayStream { pid, url }).await.map(drop)
    }

    /// Plays an input, for example `inputs/aux_in_1`. `source` is the player
    /// that has the input, if it is another player.
    pub async fn play_input(
        &self,
        pid: PlayerId,
        source: Option<PlayerId>,
        input: &str,
    ) -> Result<(), Error> {
        self.send(Command::PlayInput { pid, source, input })
            .await
            .map(drop)
    }

    /// Reads the players and their state again. Returns when all is read.
    /// It is an error if the players cannot be read.
    pub async fn refresh(&self) -> Result<(), Error> {
        let (done, read) = oneshot::channel();
        self.request(Request::Refresh(done)).await?;
        read.await.unwrap_or(Err(Error::NotConnected))
    }

    /// Stops the task and closes the connections, also while other
    /// references to this value exist. Calls after it return
    /// [`Error::NotConnected`] at once. It does not wait for the task to end,
    /// see [`Heos::close`].
    pub fn shutdown(&self) {
        let _ = self.shutdown.send(true);
    }

    /// Stops the task and closes the connections.
    pub async fn close(mut self) {
        let _ = self.shutdown.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }

    /// Sends a command and parses its answer with `parse`.
    async fn read<T>(
        &self,
        command: Command<'_>,
        parse: impl FnOnce(&Response<'_>) -> Result<T, ProtoError>,
    ) -> Result<T, Error> {
        let name = command.name();
        let answer = self.send(command).await?;
        parse(&answer.response()).map_err(|source| Error::Protocol {
            command: name,
            source,
        })
    }

    /// Passes a request to the task. A request is refused while there is no
    /// connection: it would be sent late, after a connect. It is also refused
    /// after [`Heos::shutdown`], before the task has ended.
    async fn request(&self, request: Request) -> Result<(), Error> {
        if *self.shutdown.borrow() || self.status() != HeosStatus::Connected {
            return Err(Error::NotConnected);
        }
        self.requests
            .send(request)
            .await
            .map_err(|_| Error::NotConnected)
    }
}

/// The list in the payload and the count from the message.
fn page<'a, T: Deserialize<'a>, U>(
    response: &Response<'a>,
    own: fn(T) -> U,
) -> Result<Page<U>, ProtoError> {
    Ok(Page {
        items: list(response, own)?,
        count: response.count(),
    })
}

/// The list in the payload, with `own` applied to each item. No payload is
/// an empty list.
fn list<'a, T: Deserialize<'a>, U>(
    response: &Response<'a>,
    own: fn(T) -> U,
) -> Result<Vec<U>, ProtoError> {
    match response.payload::<Vec<T>>() {
        Ok(items) => Ok(items.into_iter().map(own).collect()),
        Err(ProtoError::NoPayload) => Ok(Vec::new()),
        Err(error) => Err(error),
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
