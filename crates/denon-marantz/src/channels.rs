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
            apply(state, &mut |change| {
                if let Some(change) = change {
                    modified = true;
                    // There is an error if nobody listens. That is fine.
                    let _ = self.changes.send(change);
                }
            });
            modified
        });
    }

    pub(crate) fn watch_state(&self) -> watch::Receiver<State> {
        self.state.subscribe()
    }

    pub(crate) fn snapshot(&self) -> State {
        self.state.borrow().clone()
    }

    pub(crate) fn subscribe_changes(&self) -> broadcast::Receiver<Change> {
        self.changes.subscribe()
    }
}
