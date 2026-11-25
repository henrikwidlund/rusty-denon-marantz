//! The HEOS connection of an `Avr`: the player of the receiver, for
//! playback. Volume and mute stay on telnet and HTTP.

use std::net::IpAddr;
use std::ops::Deref;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use denon_marantz_heos::{
    BrowseItem, Change, Command, Heos, HeosConfig, HeosStatus, NowPlaying, Page, PlayMode,
    PlayState, PlayerId, PlayerState, QueueId, QueueItem, QuickSelect,
};
use tokio::sync::broadcast::error::RecvError;
use tokio::sync::{OwnedMutexGuard, broadcast, watch};
use tokio::task::{AbortHandle, JoinHandle};

use crate::error::Error;

/// State of the HEOS player of the receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HeosPlayerStatus {
    /// The receiver has no HEOS, see [`Info::has_heos`](crate::Info::has_heos).
    NotSupported,
    /// HEOS is not open.
    Disconnected,
    /// HEOS is being opened, or the connection was lost and the task
    /// connects again.
    Connecting,
    /// HEOS is open, but the device does not list a player with the address
    /// or the serial number of the receiver yet. The status changes when the
    /// list changes.
    WaitingForPlayer,
    /// The player of the receiver, see [`Avr::heos`](crate::Avr::heos).
    Ready(PlayerId),
    /// The first connection failed. Nothing keeps trying. The error is in
    /// [`Avr::heos_start_error`](crate::Avr::heos_start_error) if the start
    /// opened HEOS, else [`Avr::heos_connect`](crate::Avr::heos_connect)
    /// returned it.
    Failed(HeosFailure),
}

/// Why the first HEOS connection failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HeosFailure {
    /// The host name could not be resolved.
    NotResolved,
    /// There is no route to the receiver, for example because the network is
    /// not ready yet.
    Unreachable,
    /// The receiver refused the connection. Windows reports this only after
    /// about 2 s, so a shorter `HeosConfig::connect_timeout` gives `Timeout`
    /// there.
    Refused,
    /// The receiver did not answer in time, for example because a firewall
    /// drops the packets.
    Timeout,
    /// Another error.
    Other,
}

impl HeosFailure {
    /// The kind of a failed connect of the client.
    fn of(error: &denon_marantz_heos::Error) -> Self {
        use std::io::ErrorKind;
        match error {
            denon_marantz_heos::Error::Timeout { .. } => Self::Timeout,
            denon_marantz_heos::Error::Network { source, .. } => match source.kind() {
                ErrorKind::ConnectionRefused => Self::Refused,
                ErrorKind::TimedOut => Self::Timeout,
                ErrorKind::NetworkUnreachable
                | ErrorKind::HostUnreachable
                | ErrorKind::NetworkDown
                | ErrorKind::AddrNotAvailable => Self::Unreachable,
                _ => Self::Other,
            },
            _ => Self::Other,
        }
    }
}

/// Changes that wait for a slow subscriber of [`Avr::heos_changes`](crate::Avr::heos_changes).
const CHANGE_CAPACITY: usize = 256;

/// The HEOS player of the receiver. Get it from
/// [`Avr::heos`](crate::Avr::heos).
///
/// The `Avr` owns the connection: after
/// [`Avr::heos_disconnect`](crate::Avr::heos_disconnect), after a shutdown
/// through a kept [`HeosPlayer::client`], or when the `Avr` is dropped, the
/// calls return [`heos::Error::NotConnected`]. A bad argument gives
/// [`heos::Error::InvalidCommand`] also then.
///
/// [`heos::Error::NotConnected`]: denon_marantz_heos::Error::NotConnected
/// [`heos::Error::InvalidCommand`]: denon_marantz_heos::Error::InvalidCommand
#[derive(Debug, Clone)]
pub struct HeosPlayer {
    heos: Weak<Heos>,
    pid: PlayerId,
}

impl HeosPlayer {
    /// The player id.
    #[must_use]
    pub const fn pid(&self) -> PlayerId {
        self.pid
    }

    /// The client, for the calls that this type does not have. Its calls
    /// return [`heos::Error::NotConnected`] after
    /// [`Avr::heos_disconnect`](crate::Avr::heos_disconnect). `None` if the
    /// connection is closed or shut down. Its [`Heos::shutdown`] ends the
    /// connection of the `Avr`: the status becomes
    /// [`HeosPlayerStatus::Disconnected`], and
    /// [`Avr::heos_connect`](crate::Avr::heos_connect) opens a new one.
    ///
    /// [`heos::Error::NotConnected`]: denon_marantz_heos::Error::NotConnected
    #[must_use]
    pub fn client(&self) -> Option<Arc<Heos>> {
        self.live()
    }

    /// The state of the player: play state, what plays, play mode. `None` if
    /// the connection is closed or shut down. It is a copy:
    /// [`HeosPlayer::with_state`] reads it without one.
    #[must_use]
    pub fn state(&self) -> Option<PlayerState> {
        self.live()?.player(self.pid)
    }

    /// Runs `read` with the state of the player, without a copy. `None` if
    /// the connection is closed or shut down, or the player is not known. The
    /// state is locked while `read` runs, so keep it short.
    pub fn with_state<T>(&self, read: impl FnOnce(&PlayerState) -> T) -> Option<T> {
        self.live()?.with_player(self.pid, read)
    }

    /// What plays. `None` if nothing is known or the connection is closed or
    /// shut down.
    #[must_use]
    pub fn now_playing(&self) -> Option<NowPlaying<'static>> {
        self.state()?.current_media().cloned()
    }

    /// Plays, pauses or stops.
    ///
    /// # Errors
    ///
    /// The errors of [`Heos::set_play_state`].
    pub async fn set_play_state(&self, state: PlayState) -> Result<(), denon_marantz_heos::Error> {
        self.heos()?.set_play_state(self.pid, state).await
    }

    /// Plays the next item.
    ///
    /// # Errors
    ///
    /// The errors of [`Heos::play_next`].
    pub async fn play_next(&self) -> Result<(), denon_marantz_heos::Error> {
        self.heos()?.play_next(self.pid).await
    }

    /// Plays the previous item.
    ///
    /// # Errors
    ///
    /// The errors of [`Heos::play_previous`].
    pub async fn play_previous(&self) -> Result<(), denon_marantz_heos::Error> {
        self.heos()?.play_previous(self.pid).await
    }

    /// Sets the repeat and shuffle modes.
    ///
    /// # Errors
    ///
    /// The errors of [`Heos::set_play_mode`].
    pub async fn set_play_mode(&self, mode: PlayMode) -> Result<(), denon_marantz_heos::Error> {
        self.heos()?.set_play_mode(self.pid, mode).await
    }

    /// Reads a part of the queue, see [`Heos::queue`].
    ///
    /// # Errors
    ///
    /// The errors of [`Heos::queue`].
    pub async fn queue(
        &self,
        range: Option<(u32, u32)>,
    ) -> Result<Page<QueueItem<'static>>, denon_marantz_heos::Error> {
        self.checked(Command::GetQueue {
            pid: self.pid,
            range,
        })?
        .queue(self.pid, range)
        .await
    }

    /// Plays an item of the queue.
    ///
    /// # Errors
    ///
    /// The errors of [`Heos::play_queue_item`].
    pub async fn play_queue_item(&self, qid: QueueId) -> Result<(), denon_marantz_heos::Error> {
        self.checked(Command::PlayQueue { pid: self.pid, qid })?
            .play_queue_item(self.pid, qid)
            .await
    }

    /// Removes items from the queue. `qids` must not be empty.
    ///
    /// # Errors
    ///
    /// The errors of [`Heos::remove_from_queue`].
    pub async fn remove_from_queue(
        &self,
        qids: &[QueueId],
    ) -> Result<(), denon_marantz_heos::Error> {
        self.checked(Command::RemoveFromQueue {
            pid: self.pid,
            qids,
        })?
        .remove_from_queue(self.pid, qids)
        .await
    }

    /// Removes all items from the queue.
    ///
    /// # Errors
    ///
    /// The errors of [`Heos::clear_queue`].
    pub async fn clear_queue(&self) -> Result<(), denon_marantz_heos::Error> {
        self.heos()?.clear_queue(self.pid).await
    }

    /// Reads a part of the HEOS Favorites, see [`Heos::favorites`].
    ///
    /// # Errors
    ///
    /// The errors of [`Heos::favorites`].
    pub async fn favorites(
        &self,
        range: Option<(u32, u32)>,
    ) -> Result<Page<BrowseItem<'static>>, denon_marantz_heos::Error> {
        self.checked(Command::GetFavorites { range })?
            .favorites(range)
            .await
    }

    /// Plays a HEOS Favorite. `preset` is its position, from 1.
    ///
    /// # Errors
    ///
    /// The errors of [`Heos::play_preset`].
    pub async fn play_preset(&self, preset: u32) -> Result<(), denon_marantz_heos::Error> {
        self.checked(Command::PlayPreset {
            pid: self.pid,
            preset,
        })?
        .play_preset(self.pid, preset)
        .await
    }

    /// Reads the names of the quick select slots.
    ///
    /// # Errors
    ///
    /// The errors of [`Heos::quick_selects`].
    pub async fn quick_selects(
        &self,
    ) -> Result<Vec<QuickSelect<'static>>, denon_marantz_heos::Error> {
        self.heos()?.quick_selects(self.pid).await
    }

    /// Plays a quick select slot, from 1 to 6.
    ///
    /// # Errors
    ///
    /// The errors of [`Heos::play_quick_select`].
    pub async fn play_quick_select(&self, id: u8) -> Result<(), denon_marantz_heos::Error> {
        self.checked(Command::PlayQuickSelect { pid: self.pid, id })?
            .play_quick_select(self.pid, id)
            .await
    }

    /// As [`HeosPlayer::heos`]. Without a connection, a bad argument gives
    /// the same error as with one. `command` must be the one that the
    /// [`Heos`] method sends.
    fn checked(&self, command: Command<'_>) -> Result<Arc<Heos>, denon_marantz_heos::Error> {
        self.heos().map_err(|error| {
            command
                .write(&mut String::new())
                .map_or_else(denon_marantz_heos::Error::InvalidCommand, |()| error)
        })
    }

    fn heos(&self) -> Result<Arc<Heos>, denon_marantz_heos::Error> {
        self.live().ok_or(denon_marantz_heos::Error::NotConnected)
    }

    /// The client while its connection is not closed or shut down.
    fn live(&self) -> Option<Arc<Heos>> {
        self.heos.upgrade().filter(|heos| !heos.is_shut_down())
    }
}

/// The HEOS connection of one `Avr`.
#[derive(Debug)]
pub(crate) struct HeosLink {
    host: String,
    /// Serial number of the receiver, to find the player if the address
    /// does not match.
    serial: Option<String>,
    config: HeosConfig,
    supported: bool,
    status: Arc<watch::Sender<HeosPlayerStatus>>,
    /// The changes of all connections, see `Avr::heos_changes`.
    changes: broadcast::Sender<Change>,
    /// Lets one connect or disconnect run at a time.
    gate: Arc<tokio::sync::Mutex<()>>,
    running: Mutex<Option<Running>>,
    /// The error of the last connect that the start ran in the background.
    start_error: Mutex<Option<Arc<Error>>>,
}

/// An open connection and the task that finds the player.
#[derive(Debug)]
struct Running {
    heos: Arc<Heos>,
    watcher: JoinHandle<()>,
}

impl Running {
    /// False after a shutdown of the client, for example through a kept
    /// client, or after the task of the client or the watcher ended.
    fn is_alive(&self) -> bool {
        !self.heos.is_shut_down()
            && !self.watcher.is_finished()
            && self.heos.watch_players().has_changed().is_ok()
    }
}

impl HeosLink {
    pub(crate) fn new(
        host: String,
        serial: Option<String>,
        config: HeosConfig,
        supported: bool,
    ) -> Self {
        let first = if supported {
            HeosPlayerStatus::Disconnected
        } else {
            HeosPlayerStatus::NotSupported
        };
        Self {
            host,
            serial,
            config,
            supported,
            status: Arc::new(watch::Sender::new(first)),
            changes: broadcast::Sender::new(CHANGE_CAPACITY),
            gate: Arc::new(tokio::sync::Mutex::new(())),
            running: Mutex::new(None),
            start_error: Mutex::new(None),
        }
    }

    /// See [`HeosOwner::connect_in_background`].
    pub(crate) fn start_error(&self) -> Option<Arc<Error>> {
        self.start_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Opens the connection and starts the task that finds the player.
    /// Returns false if a connection is open already. A connection whose task
    /// ended counts as closed.
    pub(crate) async fn connect(&self) -> Result<bool, Error> {
        let gate = Arc::clone(&self.gate).lock_owned().await;
        self.open(gate, |error| error).await
    }

    /// As [`HeosLink::connect`], with `gate` held. `failed` takes the error
    /// before the status says that the connect failed.
    async fn open<E>(
        &self,
        _gate: OwnedMutexGuard<()>,
        failed: impl FnOnce(Error) -> E,
    ) -> Result<bool, E> {
        if !self.supported {
            return Err(failed(Error::HeosNotSupported));
        }
        let dead = {
            let mut running = self.lock();
            if running.as_ref().is_some_and(Running::is_alive) {
                return Ok(false);
            }
            running.take()
        };
        *self
            .start_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        set_status(&self.status, HeosPlayerStatus::Connecting);
        // Sets the status back if the future is dropped.
        let mut guard = StatusGuard(Some(&self.status));
        // A connection whose task ended, for example after a shutdown through
        // a kept client, is replaced. Its watcher must end first, so that it
        // cannot set the status later.
        if let Some(Running { heos, watcher }) = dead {
            watcher.abort();
            heos.shutdown();
            let _ = watcher.await;
            // The watcher can have set it during the wait.
            set_status(&self.status, HeosPlayerStatus::Connecting);
        }
        let opened = match self.addresses().await {
            Ok(addresses) => Heos::connect_with(self.host.as_str(), self.config)
                .await
                .map(|heos| (addresses, Arc::new(heos)))
                .map_err(|error| (HeosFailure::of(&error), Error::Heos(error))),
            Err(error) => Err((HeosFailure::NotResolved, error)),
        };
        let (addresses, heos) = match opened {
            Ok(opened) => opened,
            Err((failure, error)) => {
                let error = failed(error);
                guard.0 = None;
                set_status(&self.status, HeosPlayerStatus::Failed(failure));
                return Err(error);
            }
        };
        // The lock is held until the connection is stored, so that
        // `player` finds it when the status is ready.
        let mut running = self.lock();
        let watcher = tokio::spawn(watch_player(
            Watched {
                heos: Arc::downgrade(&heos),
                connection: heos.watch_status(),
                players: heos.watch_players(),
                changes: heos.changes(),
            },
            Match {
                addresses,
                serial: self.serial.clone(),
            },
            Arc::clone(&self.status),
            self.changes.clone(),
        ));
        *running = Some(Running { heos, watcher });
        drop(running);
        guard.0 = None;
        Ok(true)
    }

    /// Closes the connection, also for a kept [`HeosPlayer::client`].
    async fn close(&self) {
        let _gate = self.gate.lock().await;
        let running = self.lock().take();
        *self
            .start_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        // Before the wait, so the status is right in most cases if this future
        // is dropped. A watcher that runs on another thread can still set it
        // once.
        if self.supported {
            set_status(&self.status, HeosPlayerStatus::Disconnected);
        }
        if let Some(Running { heos, watcher }) = running {
            watcher.abort();
            heos.shutdown();
            // The watcher can set the status until it has ended.
            let _ = watcher.await;
            if self.supported {
                set_status(&self.status, HeosPlayerStatus::Disconnected);
            }
            // Waits for the task if no client is kept.
            if let Some(heos) = Arc::into_inner(heos) {
                heos.close().await;
            }
        }
    }

    pub(crate) fn watch_status(&self) -> watch::Receiver<HeosPlayerStatus> {
        self.status.subscribe()
    }

    pub(crate) fn changes(&self) -> broadcast::Receiver<Change> {
        self.changes.subscribe()
    }

    pub(crate) fn player(&self) -> Option<HeosPlayer> {
        let HeosPlayerStatus::Ready(pid) = *self.status.borrow() else {
            return None;
        };
        self.lock().as_ref().map(|running| HeosPlayer {
            heos: Arc::downgrade(&running.heos),
            pid,
        })
    }

    /// The IP addresses of the host, an IPv4-mapped IPv6 address as IPv4. A
    /// host name is resolved once per connect, within the connect timeout.
    async fn addresses(&self) -> Result<Vec<IpAddr>, Error> {
        if let Ok(ip) = self.host.parse::<IpAddr>() {
            return Ok(vec![ip.to_canonical()]);
        }
        let request = || format!("HEOS resolve {}", self.host);
        let limit = self.config.connect_timeout;
        let found = tokio::time::timeout(limit, tokio::net::lookup_host((self.host.as_str(), 0)))
            .await
            .map_err(|_| Error::Timeout {
                request: request(),
                limit,
                sent: false,
            })?
            .map_err(|source| Error::Network {
                request: request(),
                source: Box::new(source),
                sent: false,
            })?;
        Ok(found.map(|address| address.ip().to_canonical()).collect())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<Running>> {
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The [`HeosLink`] of an `Avr`. Stops the connect of the start when it is
/// dropped or when HEOS is closed.
#[derive(Debug)]
pub(crate) struct HeosOwner {
    link: Arc<HeosLink>,
    /// The task of [`HeosOwner::connect_in_background`].
    start_task: Mutex<Option<AbortHandle>>,
}

impl HeosOwner {
    pub(crate) fn new(link: HeosLink) -> Self {
        Self {
            link: Arc::new(link),
            start_task: Mutex::new(None),
        }
    }

    /// Opens the connection in a task, for the start. Returns false if a
    /// connection is open or being opened already. A failure is kept for
    /// [`HeosLink::start_error`]. The task does not keep the `Avr`.
    pub(crate) fn connect_in_background(&self) -> bool {
        // The task keeps the gate, so that a disconnect cannot run before it.
        let Ok(gate) = Arc::clone(&self.link.gate).try_lock_owned() else {
            return false;
        };
        // With the gate held, so that a connect cannot end in between.
        if self.link.lock().as_ref().is_some_and(Running::is_alive) {
            return false;
        }
        // At once, so that the status is not `Disconnected` after the start.
        // Right after a shutdown through a kept client, the old watcher can
        // still set `Disconnected` once before the task sets `Connecting`
        // again.
        set_status(&self.link.status, HeosPlayerStatus::Connecting);
        let link = Arc::clone(&self.link);
        // Held until the handle is stored, so that `disconnect` finds it.
        let mut start_task = self.start_task();
        let task = tokio::spawn(async move {
            // The error is kept before the status says that it failed.
            let _ = link
                .open(gate, |error| {
                    *link
                        .start_error
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner) = Some(Arc::new(error));
                })
                .await;
        });
        *start_task = Some(task.abort_handle());
        true
    }

    /// Stops the connect of the start, then closes the connection.
    pub(crate) async fn disconnect(&self) {
        self.stop_start();
        self.link.close().await;
    }

    fn stop_start(&self) {
        let task = self.start_task().take();
        if let Some(task) = task {
            task.abort();
        }
    }

    fn start_task(&self) -> std::sync::MutexGuard<'_, Option<AbortHandle>> {
        self.start_task
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

impl Deref for HeosOwner {
    type Target = HeosLink;

    fn deref(&self) -> &HeosLink {
        &self.link
    }
}

impl Drop for HeosOwner {
    fn drop(&mut self) {
        self.stop_start();
    }
}

impl Drop for HeosLink {
    fn drop(&mut self) {
        let running = self.lock().take();
        if let Some(running) = running {
            running.watcher.abort();
            running.heos.shutdown();
        }
        if self.supported {
            set_status(&self.status, HeosPlayerStatus::Disconnected);
        }
    }
}

/// What the task watches on one connection.
struct Watched {
    /// To see a shutdown before the task of the client ends.
    heos: Weak<Heos>,
    connection: watch::Receiver<HeosStatus>,
    players: watch::Receiver<Vec<PlayerState>>,
    changes: broadcast::Receiver<Change>,
}

/// How the player of the receiver is found: by its address, else by the
/// serial number.
struct Match {
    addresses: Vec<IpAddr>,
    serial: Option<String>,
}

impl Match {
    fn find(&self, players: &[PlayerState]) -> Option<PlayerId> {
        let by_address = players.iter().find(|state| {
            state
                .player
                .ip_addr()
                .is_some_and(|ip| self.addresses.contains(&ip.to_canonical()))
        });
        by_address
            .or_else(|| {
                let serial = self.serial.as_deref()?;
                players
                    .iter()
                    .find(|state| state.player.serial.as_deref() == Some(serial))
            })
            .map(PlayerState::pid)
    }
}

/// Sets the status of the player from the connection and the players, each
/// time one of them changes, and passes on the changes. Ends when the
/// connection is gone.
async fn watch_player(
    mut watched: Watched,
    found: Match,
    status: Arc<watch::Sender<HeosPlayerStatus>>,
    forward: broadcast::Sender<Change>,
) {
    let mut update = true;
    loop {
        if update {
            // The client was shut down, or its task ended.
            if watched.players.has_changed().is_err()
                || watched
                    .heos
                    .upgrade()
                    .is_none_or(|heos| heos.is_shut_down())
            {
                set_status(&status, HeosPlayerStatus::Disconnected);
                return;
            }
            let new = if *watched.connection.borrow_and_update() == HeosStatus::Connected {
                found
                    .find(&watched.players.borrow_and_update())
                    .map_or(HeosPlayerStatus::WaitingForPlayer, HeosPlayerStatus::Ready)
            } else {
                HeosPlayerStatus::Connecting
            };
            set_status(&status, new);
        }
        update = true;
        tokio::select! {
            // The status first, so that it is set before a change of the
            // players is passed on.
            biased;
            changed = watched.connection.changed() => if changed.is_err() {
                set_status(&status, HeosPlayerStatus::Disconnected);
                return;
            },
            // The task of the client ended, for example after a shutdown
            // through a kept client.
            changed = watched.players.changed() => if changed.is_err() {
                set_status(&status, HeosPlayerStatus::Disconnected);
                return;
            },
            change = watched.changes.recv() => {
                update = false;
                match change {
                    // No subscriber is not an error.
                    Ok(change) => drop(forward.send(change)),
                    // Changes were lost: subscribers read everything again.
                    Err(RecvError::Lagged(_)) => {
                        let changes = all_changes(&watched.players.borrow());
                        for change in changes {
                            drop(forward.send(change));
                        }
                    }
                    Err(RecvError::Closed) => {
                        set_status(&status, HeosPlayerStatus::Disconnected);
                        return;
                    }
                }
            }
        }
    }
}

/// A change of each kind for each player, so that a subscriber reads the
/// whole state again.
fn all_changes(players: &[PlayerState]) -> Vec<Change> {
    let mut changes = vec![Change::Players];
    for pid in players.iter().map(PlayerState::pid) {
        changes.extend([
            Change::PlayState(pid),
            Change::NowPlaying(pid),
            Change::Progress(pid),
            Change::PlayMode(pid),
            Change::Queue(pid),
        ]);
    }
    changes
}

/// Sets the status. Watchers are told only if it changed.
fn set_status(status: &watch::Sender<HeosPlayerStatus>, new: HeosPlayerStatus) {
    status.send_if_modified(|current| {
        let changed = *current != new;
        *current = new;
        changed
    });
}

/// Sets the status to disconnected when a connect does not finish. `None`
/// after the connect worked.
struct StatusGuard<'a>(Option<&'a watch::Sender<HeosPlayerStatus>>);

impl Drop for StatusGuard<'_> {
    fn drop(&mut self) {
        if let Some(status) = self.0 {
            set_status(status, HeosPlayerStatus::Disconnected);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;
    use std::time::Duration;

    use super::*;

    #[test]
    fn a_failed_connect_has_its_kind() {
        let network = |kind| denon_marantz_heos::Error::Network {
            request: "connect".to_owned(),
            source: std::io::Error::from(kind),
        };
        let timeout = denon_marantz_heos::Error::Timeout {
            request: "connect".to_owned(),
            limit: Duration::from_secs(5),
            sent: false,
        };
        let cases = [
            (timeout, HeosFailure::Timeout),
            (network(ErrorKind::ConnectionRefused), HeosFailure::Refused),
            (network(ErrorKind::TimedOut), HeosFailure::Timeout),
            (
                network(ErrorKind::HostUnreachable),
                HeosFailure::Unreachable,
            ),
            (
                network(ErrorKind::NetworkUnreachable),
                HeosFailure::Unreachable,
            ),
            (network(ErrorKind::NetworkDown), HeosFailure::Unreachable),
            (
                network(ErrorKind::AddrNotAvailable),
                HeosFailure::Unreachable,
            ),
            (network(ErrorKind::Other), HeosFailure::Other),
            (denon_marantz_heos::Error::NotConnected, HeosFailure::Other),
        ];
        for (error, kind) in cases {
            assert_eq!(HeosFailure::of(&error), kind, "{error:?}");
        }
    }

    #[tokio::test]
    async fn a_mapped_ipv6_host_is_used_as_ipv4() {
        let link = |host: &str| HeosLink::new(host.to_owned(), None, HeosConfig::default(), true);
        let v4: IpAddr = "192.0.2.5".parse().unwrap();
        for host in ["::ffff:192.0.2.5", "192.0.2.5"] {
            assert_eq!(link(host).addresses().await.unwrap(), [v4], "{host}");
        }
        let v6: IpAddr = "2001:db8::5".parse().unwrap();
        assert_eq!(link("2001:db8::5").addresses().await.unwrap(), [v6]);
    }
}
