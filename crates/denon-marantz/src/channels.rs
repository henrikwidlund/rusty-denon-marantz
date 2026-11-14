//! Where the state and its changes are published.

use denon_marantz_proto::{Change, State};
use tokio::sync::{broadcast, watch};

/// Changes that a slow subscriber can fall behind before it loses some. One
/// read of all zones can report more than 64 changes.
const CHANGE_CAPACITY: usize = 256;

/// The state of a receiver and the stream of its changes.
///
/// Every source of values (HTTP updates, telnet events) changes the state
/// through [`Channels::modify`], so all of them tell watchers the same way.
#[derive(Debug, Clone)]
pub(crate) struct Channels {
    state: watch::Sender<State>,
    changes: broadcast::Sender<Change>,
}

impl Channels {
    pub(crate) fn new(state: State) -> Self {
        Self {
            state: watch::Sender::new(state),
            changes: broadcast::Sender::new(CHANGE_CAPACITY),
        }
    }

    /// Changes the state with `apply`. It reports each change it makes to the
    /// function that it gets. Watchers are told once if anything changed, and
    /// each change goes to the subscribers of [`Channels::subscribe_changes`].
    pub(crate) fn modify(&self, apply: impl FnOnce(&mut State, &mut dyn FnMut(Option<Change>))) {
        self.state.send_if_modified(|state| {
            let mut modified = false;
            let mut publish = |change: Change| {
                modified = true;
                // There is an error if nobody listens. That is fine.
                let _ = self.changes.send(change);
            };
            apply(state, &mut |change| {
                if let Some(change) = change {
                    publish(change);
                }
            });
            for change in state.take_follow_ups() {
                publish(change);
            }
            modified
        });
    }

    pub(crate) fn watch_state(&self) -> watch::Receiver<State> {
        self.state.subscribe()
    }

    /// Reads the state without a copy.
    #[cfg(feature = "telnet")]
    pub(crate) fn read<T>(&self, read: impl FnOnce(&State) -> T) -> T {
        read(&self.state.borrow())
    }

    pub(crate) fn snapshot(&self) -> State {
        self.state.borrow().clone()
    }

    pub(crate) fn subscribe_changes(&self) -> broadcast::Receiver<Change> {
        self.changes.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use denon_marantz_proto::telnet::Message;
    use denon_marantz_proto::{Setting, Zone};

    use super::*;

    fn apply(channels: &Channels, line: &str) {
        channels.modify(|state, notify| {
            notify(state.apply_message(&Message::parse(line).unwrap()));
        });
    }

    #[test]
    fn a_follow_up_change_is_published_after_the_change() {
        let channels = Channels::new(State::new(false));
        apply(&channels, "SIMPLAY");
        apply(&channels, "NSE1Song");
        let mut changes = channels.subscribe_changes();
        apply(&channels, "SIGAME");
        let first = changes.try_recv().unwrap();
        assert_eq!(
            (first.zone, first.setting),
            (Some(Zone::Main), Setting::Source)
        );
        let second = changes.try_recv().unwrap();
        assert_eq!((second.zone, second.setting), (None, Setting::NowPlaying));
        assert!(changes.try_recv().is_err());
    }
}
