//! The state of the players, and where its changes are published.

use std::sync::Arc;

use denon_marantz_heos_proto::{
    Controls, Event, NowPlaying, PlayMode, PlayState, Player, PlayerId, Progress, RepeatMode,
};
use tokio::sync::{broadcast, watch};
use tokio::time::Instant;

/// Changes that a slow subscriber can fall behind before it loses some.
const CHANGE_CAPACITY: usize = 256;

/// Events that a slow subscriber can fall behind before it loses some.
const EVENT_CAPACITY: usize = 256;

/// State of the HEOS connection.
///
/// While there is no connection, the state of the players keeps the last
/// values. After a connect, they are read again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum HeosStatus {
    /// There is no connection. A task that is running tries to connect again.
    Disconnected,
    /// The connections are open.
    Connected,
}

/// State of one player.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlayerState {
    /// The player, from `get_players`.
    pub player: Arc<Player<'static>>,
    /// Play state. `None` until it is read, and while the device reports it
    /// as not known.
    pub play_state: Option<PlayState>,
    /// What plays. `None` until it is read, when the device reports no item,
    /// and while the item has no text: `AirPlay` sends such an item when it
    /// starts, before the title.
    ///
    /// A stopped player keeps its last item, also after the receiver changed
    /// to a source that is not HEOS. [`PlayerState::current_media`] hides it.
    /// While the play state is not known, the item is shown, but it can be
    /// old: the device also reports this state for a station that plays.
    pub now_playing: Option<Arc<NowPlaying<'static>>>,
    /// Position in what plays, from the last progress event. `None` until
    /// the first event, after what plays changed, after a stop, and when the
    /// play state becomes not known.
    ///
    /// In the first seconds after `AirPlay` starts, the position can jump.
    /// A progress event with a position or length that is not valid (a
    /// negative number that the device sends without a sign) is skipped.
    pub progress: Option<Progress>,
    /// When [`PlayerState::progress`] last changed. The device sends the
    /// position every few seconds: while the player plays, add the time since
    /// then to the position.
    pub progress_at: Option<Instant>,
    /// Repeat mode. `None` until it is read.
    pub repeat: Option<RepeatMode>,
    /// Shuffle is on. `None` until it is read.
    pub shuffle: Option<bool>,
}

impl PlayerState {
    fn new(player: Player<'static>) -> Self {
        Self {
            player: Arc::new(player),
            play_state: None,
            now_playing: None,
            progress: None,
            progress_at: None,
            repeat: None,
            shuffle: None,
        }
    }

    /// Clears the progress and its time. Returns true if there was one.
    const fn clear_progress(&mut self) -> bool {
        self.progress_at = None;
        self.progress.take().is_some()
    }

    /// Player id.
    #[must_use]
    pub fn pid(&self) -> PlayerId {
        self.player.pid
    }

    /// Repeat and shuffle modes, or `None` until both are read.
    #[must_use]
    pub fn play_mode(&self) -> Option<PlayMode> {
        Some(PlayMode {
            repeat: self.repeat?,
            shuffle: self.shuffle?,
        })
    }

    /// What plays, or `None` while the player is stopped. While the play
    /// state is not known, the item is shown.
    #[must_use]
    pub fn current_media(&self) -> Option<&NowPlaying<'static>> {
        if self.play_state == Some(PlayState::Stop) {
            None
        } else {
            self.now_playing.as_deref()
        }
    }

    /// Transport controls that work for [`PlayerState::now_playing`]. All
    /// controls if there is no item.
    ///
    /// While the player is stopped, they are the controls of the item that
    /// play would start, which can be another item than the one that played
    /// last. For example, after `AirPlay` stops, the item is the last queue
    /// item, such as a radio station.
    #[must_use]
    pub fn controls(&self) -> Controls {
        self.now_playing.as_deref().map_or(Controls::ALL, |media| {
            Controls::for_media(media.sid, media.media_type)
        })
    }
}

/// A change of the state.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Change {
    /// Players were added or removed, or their details changed.
    Players,
    /// The players could not be read. The list of players stays as it was.
    /// The reason is in the debug log.
    PlayersNotRead,
    /// The play state of a player changed.
    PlayState(PlayerId),
    /// What a player plays changed.
    NowPlaying(PlayerId),
    /// The position in what a player plays changed.
    Progress(PlayerId),
    /// The repeat or shuffle mode of a player changed.
    PlayMode(PlayerId),
    /// The queue of a player changed, or may have changed while there was no
    /// connection. The state does not keep the queue: read it again with
    /// [`Heos::queue`](crate::Heos::queue).
    Queue(PlayerId),
    /// Playback failed. The state does not keep this.
    PlaybackError {
        /// Player.
        pid: PlayerId,
        /// Error text that can be shown to the user.
        error: Box<str>,
    },
}

/// The state, its changes, the events and the connection status.
#[derive(Debug, Clone)]
pub(crate) struct Shared {
    players: watch::Sender<Vec<PlayerState>>,
    changes: broadcast::Sender<Change>,
    events: broadcast::Sender<Event<'static>>,
    status: watch::Sender<HeosStatus>,
}

impl Shared {
    pub(crate) fn new() -> Self {
        Self {
            players: watch::Sender::new(Vec::new()),
            changes: broadcast::Sender::new(CHANGE_CAPACITY),
            events: broadcast::Sender::new(EVENT_CAPACITY),
            status: watch::Sender::new(HeosStatus::Disconnected),
        }
    }

    /// What a client reads. It does not keep the senders of the players,
    /// changes and events, so their receivers close when the task ends.
    pub(crate) fn view(&self) -> View {
        View {
            players: self.players.subscribe(),
            changes: self.changes.downgrade(),
            events: self.events.downgrade(),
            status: self.status.clone(),
        }
    }

    /// Reads the players without a copy.
    pub(crate) fn read<T>(&self, read: impl FnOnce(&[PlayerState]) -> T) -> T {
        read(&self.players.borrow())
    }

    /// Sets the status. Watchers are told only if it changed.
    pub(crate) fn set_status(&self, new: HeosStatus) {
        self.status.send_if_modified(|current| set(current, new));
    }

    pub(crate) fn publish(&self, change: Change) {
        // There is an error if nobody listens. That is fine.
        let _ = self.changes.send(change);
    }

    /// Sends the event to its subscribers. It is copied only if there is one.
    pub(crate) fn send_event(&self, event: Event<'_>) {
        if self.events.receiver_count() > 0 {
            let _ = self.events.send(event.into_owned());
        }
    }

    /// Replaces the list of players. Players that stay keep their state. Of
    /// players with the same id, only the first is kept.
    pub(crate) fn set_players(&self, list: Vec<Player<'_>>) {
        let list = first_of_each_pid(list);
        let modified = self.players.send_if_modified(|states| {
            let same = states.len() == list.len()
                && states
                    .iter()
                    .zip(&list)
                    .all(|(state, player)| *state.player == *player);
            if same {
                return false;
            }
            let mut old = std::mem::take(states);
            for player in list {
                let state = match old.iter().position(|state| state.pid() == player.pid) {
                    Some(index) => {
                        let mut state = old.swap_remove(index);
                        if *state.player != player {
                            state.player = Arc::new(player.into_owned());
                        }
                        state
                    }
                    None => PlayerState::new(player.into_owned()),
                };
                states.push(state);
            }
            true
        });
        if modified {
            self.publish(Change::Players);
        }
    }

    /// A stop or a state that is not known (`None`) also clears the progress.
    /// The device sends no progress event for it.
    pub(crate) fn set_play_state(&self, pid: PlayerId, play_state: Option<PlayState>) {
        self.update(pid, |state| {
            let cleared =
                matches!(play_state, None | Some(PlayState::Stop)) && state.clear_progress();
            let changed = set(&mut state.play_state, play_state);
            [
                changed.then_some(Change::PlayState(pid)),
                cleared.then_some(Change::Progress(pid)),
            ]
        });
    }

    /// New media also clears the progress.
    pub(crate) fn set_now_playing(&self, pid: PlayerId, media: Option<NowPlaying<'_>>) {
        self.update(pid, |state| {
            if state.now_playing.as_deref() == media.as_ref() {
                return [None, None];
            }
            state.now_playing = media.map(|media| Arc::new(media.into_owned()));
            let cleared = state.clear_progress();
            [
                Some(Change::NowPlaying(pid)),
                cleared.then_some(Change::Progress(pid)),
            ]
        });
    }

    pub(crate) fn set_progress(&self, pid: PlayerId, progress: Progress) {
        self.update(pid, |state| {
            let changed = set(&mut state.progress, Some(progress));
            if changed {
                state.progress_at = Some(Instant::now());
            }
            [changed.then_some(Change::Progress(pid)), None]
        });
    }

    /// Sets the repeat mode, the shuffle mode, or both.
    pub(crate) fn set_play_mode(
        &self,
        pid: PlayerId,
        repeat: Option<RepeatMode>,
        shuffle: Option<bool>,
    ) {
        self.update(pid, |state| {
            let mut changed = false;
            if let Some(repeat) = repeat {
                changed |= set(&mut state.repeat, Some(repeat));
            }
            if let Some(shuffle) = shuffle {
                changed |= set(&mut state.shuffle, Some(shuffle));
            }
            [changed.then_some(Change::PlayMode(pid)), None]
        });
    }

    /// Changes one player with `apply`, which returns the changes that it
    /// made. They are published after the state is changed.
    fn update(&self, pid: PlayerId, apply: impl FnOnce(&mut PlayerState) -> [Option<Change>; 2]) {
        let mut changes = [None, None];
        self.players.send_if_modified(|states| {
            if let Some(state) = states.iter_mut().find(|state| state.pid() == pid) {
                changes = apply(state);
            }
            changes.iter().any(Option::is_some)
        });
        for change in changes.into_iter().flatten() {
            self.publish(change);
        }
    }
}

/// The side of [`Shared`] that a client keeps. The status stays readable
/// after the task ends.
#[derive(Debug)]
pub(crate) struct View {
    players: watch::Receiver<Vec<PlayerState>>,
    changes: broadcast::WeakSender<Change>,
    events: broadcast::WeakSender<Event<'static>>,
    status: watch::Sender<HeosStatus>,
}

impl View {
    /// Reads the players without a copy.
    pub(crate) fn read<T>(&self, read: impl FnOnce(&[PlayerState]) -> T) -> T {
        read(&self.players.borrow())
    }

    /// Closes when the task ends.
    pub(crate) fn watch_players(&self) -> watch::Receiver<Vec<PlayerState>> {
        let mut players = self.players.clone();
        players.mark_unchanged();
        players
    }

    /// Closes when the task ends. Closed at once after it.
    pub(crate) fn subscribe_changes(&self) -> broadcast::Receiver<Change> {
        subscribe(&self.changes)
    }

    /// Closes when the task ends. Closed at once after it.
    pub(crate) fn subscribe_events(&self) -> broadcast::Receiver<Event<'static>> {
        subscribe(&self.events)
    }

    pub(crate) fn status(&self) -> HeosStatus {
        *self.status.borrow()
    }

    pub(crate) fn watch_status(&self) -> watch::Receiver<HeosStatus> {
        self.status.subscribe()
    }
}

/// A receiver of `sender`, or a closed one if the sender is gone.
fn subscribe<T: Clone>(sender: &broadcast::WeakSender<T>) -> broadcast::Receiver<T> {
    sender
        .upgrade()
        .map_or_else(|| broadcast::channel(1).1, |sender| sender.subscribe())
}

/// Keeps the first player of each id, in order.
fn first_of_each_pid(mut list: Vec<Player<'_>>) -> Vec<Player<'_>> {
    let mut kept = 0;
    for index in 0..list.len() {
        let pid = list[index].pid;
        if !list[..kept].iter().any(|player| player.pid == pid) {
            list.swap(kept, index);
            kept += 1;
        }
    }
    list.truncate(kept);
    list
}

/// Sets `slot` to `value`. Returns true if it changed.
fn set<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        false
    } else {
        *slot = value;
        true
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use denon_marantz_heos_proto::{MediaType, SourceId};

    use super::*;

    fn player(pid: i64, name: &str) -> Player<'_> {
        Player {
            name: name.into(),
            pid: PlayerId(pid),
            gid: None,
            model: None,
            version: None,
            ip: None,
            network: None,
            lineout: None,
            control: None,
            serial: None,
        }
    }

    fn media(sid: SourceId, song: &str) -> NowPlaying<'_> {
        NowPlaying {
            media_type: MediaType::Station,
            song: song.into(),
            station: None,
            album: "".into(),
            artist: "".into(),
            image_url: "".into(),
            album_id: None,
            mid: None,
            qid: None,
            sid: Some(sid),
        }
    }

    fn changes(receiver: &mut broadcast::Receiver<Change>) -> Vec<Change> {
        std::iter::from_fn(|| receiver.try_recv().ok()).collect()
    }

    #[test]
    fn players_keep_their_state() {
        let shared = Shared::new();
        let mut receiver = shared.view().subscribe_changes();
        shared.set_players(vec![player(1, "A"), player(2, "B")]);
        shared.set_play_state(PlayerId(2), Some(PlayState::Play));
        shared.set_players(vec![player(2, "B"), player(1, "A")]);
        shared.set_players(vec![player(2, "B"), player(1, "A")]);
        shared.set_players(vec![player(2, "C")]);
        assert_eq!(
            changes(&mut receiver),
            [
                Change::Players,
                Change::PlayState(PlayerId(2)),
                Change::Players,
                Change::Players,
            ]
        );
        shared.read(|states| {
            assert_eq!(states.len(), 1);
            assert_eq!(states[0].player.name, "C");
            assert_eq!(states[0].play_state, Some(PlayState::Play));
        });
    }

    #[test]
    fn only_real_changes_are_published() {
        let shared = Shared::new();
        let pid = PlayerId(1);
        shared.set_players(vec![player(1, "A")]);
        let mut receiver = shared.view().subscribe_changes();
        let progress = Progress {
            position_ms: 1000,
            duration_ms: 0,
        };
        shared.set_progress(pid, progress);
        shared.set_progress(pid, progress);
        shared.set_now_playing(pid, Some(media(SourceId::TUNEIN, "A")));
        shared.set_now_playing(pid, Some(media(SourceId::TUNEIN, "A")));
        // A player that is not known is ignored.
        shared.set_play_state(PlayerId(9), Some(PlayState::Play));
        // New media clears the progress.
        assert_eq!(
            changes(&mut receiver),
            [
                Change::Progress(pid),
                Change::NowPlaying(pid),
                Change::Progress(pid)
            ]
        );
        shared.read(|states| {
            // New media resets the progress.
            assert_eq!(states[0].progress, None);
            assert_eq!(
                states[0].controls(),
                Controls::for_media(Some(SourceId::TUNEIN), MediaType::Station)
            );
        });
    }

    #[test]
    fn only_the_first_player_of_an_id_is_kept() {
        let shared = Shared::new();
        let mut receiver = shared.view().subscribe_changes();
        let list = || {
            vec![
                player(1, "A"),
                player(2, "B"),
                player(1, "C"),
                player(2, "D"),
            ]
        };
        shared.set_players(list());
        shared.set_players(list());
        assert_eq!(changes(&mut receiver), [Change::Players]);
        shared.read(|states| {
            let names: Vec<_> = states.iter().map(|state| &*state.player.name).collect();
            assert_eq!(names, ["A", "B"]);
        });
    }

    #[test]
    fn stop_clears_the_progress() {
        let shared = Shared::new();
        let pid = PlayerId(1);
        shared.set_players(vec![player(1, "A")]);
        let progress = Progress {
            position_ms: 1000,
            duration_ms: 5000,
        };
        shared.set_progress(pid, progress);
        shared.set_play_state(pid, Some(PlayState::Pause));
        shared.read(|states| assert_eq!(states[0].progress, Some(progress)));
        let mut receiver = shared.view().subscribe_changes();
        shared.set_play_state(pid, Some(PlayState::Stop));
        shared.read(|states| assert_eq!(states[0].progress, None));
        assert_eq!(
            changes(&mut receiver),
            [Change::PlayState(pid), Change::Progress(pid)]
        );
        // Without a progress, a stop is one change.
        shared.set_play_state(pid, Some(PlayState::Play));
        shared.set_play_state(pid, Some(PlayState::Stop));
        assert_eq!(
            changes(&mut receiver),
            [Change::PlayState(pid), Change::PlayState(pid)]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn progress_has_the_time_of_its_last_change() {
        let shared = Shared::new();
        let pid = PlayerId(1);
        shared.set_players(vec![player(1, "A")]);
        let at = |position_ms| Progress {
            position_ms,
            duration_ms: 5000,
        };
        let time = || shared.read(|states| states[0].progress_at);
        let start = Instant::now();
        shared.set_progress(pid, at(1000));
        assert_eq!(time(), Some(start));
        // The same position again keeps the time.
        tokio::time::advance(Duration::from_secs(3)).await;
        shared.set_progress(pid, at(1000));
        assert_eq!(time(), Some(start));
        shared.set_progress(pid, at(4000));
        assert_eq!(time(), Some(start + Duration::from_secs(3)));
        shared.set_play_state(pid, Some(PlayState::Stop));
        assert_eq!(time(), None);
        shared.set_progress(pid, at(0));
        shared.set_now_playing(pid, Some(media(SourceId::TUNEIN, "B")));
        assert_eq!(time(), None);
    }

    #[test]
    fn play_mode_needs_both_values() {
        let shared = Shared::new();
        let pid = PlayerId(1);
        shared.set_players(vec![player(1, "A")]);
        let mut receiver = shared.view().subscribe_changes();
        shared.set_play_mode(pid, Some(RepeatMode::OnAll), None);
        shared.read(|states| assert_eq!(states[0].play_mode(), None));
        shared.set_play_mode(pid, None, Some(true));
        shared.set_play_mode(pid, Some(RepeatMode::OnAll), Some(true));
        shared.read(|states| {
            assert_eq!(
                states[0].play_mode(),
                Some(PlayMode {
                    repeat: RepeatMode::OnAll,
                    shuffle: true,
                })
            );
        });
        assert_eq!(
            changes(&mut receiver),
            [Change::PlayMode(pid), Change::PlayMode(pid)]
        );
    }

    #[test]
    fn stopped_player_hides_its_media() {
        let shared = Shared::new();
        let pid = PlayerId(1);
        shared.set_players(vec![player(1, "A")]);
        shared.set_now_playing(pid, Some(media(SourceId::LOCAL_MUSIC, "A")));
        shared.set_play_state(pid, Some(PlayState::Stop));
        shared.read(|states| {
            assert!(states[0].now_playing.is_some());
            assert_eq!(states[0].current_media(), None);
        });
        shared.set_play_state(pid, Some(PlayState::Pause));
        shared.read(|states| assert!(states[0].current_media().is_some()));
    }

    #[test]
    fn stopped_player_has_the_controls_of_the_item_that_play_starts() {
        let shared = Shared::new();
        let pid = PlayerId(1);
        shared.set_players(vec![player(1, "A")]);
        // After AirPlay stops, the item is the last queue item.
        shared.set_play_state(pid, Some(PlayState::Stop));
        shared.set_now_playing(pid, Some(media(SourceId::TUNEIN, "Radio")));
        shared.read(|states| {
            assert_eq!(states[0].current_media(), None);
            assert_eq!(
                states[0].controls(),
                Controls::for_media(Some(SourceId::TUNEIN), MediaType::Station)
            );
        });
    }
}
