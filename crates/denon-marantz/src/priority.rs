//! Commands over HTTP go before the reads.

use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tokio::sync::{Mutex as OrderLock, MutexGuard as OrderGuard, Notify};
use tokio::time::Instant;

/// Makes HTTP reads wait while a command waits or runs, and for a pause after
/// the last command. One value is shared by all HTTP clients of one receiver.
/// Commands go one after the other, in the order of the calls.
#[derive(Debug)]
pub(crate) struct Priority {
    pause: Duration,
    state: Mutex<State>,
    /// Woken when the last command that waits or runs ends.
    idle: Notify,
    /// Keeps the order of the commands.
    order: OrderLock<()>,
}

#[derive(Debug)]
struct State {
    /// Commands that wait or run.
    commands: usize,
    /// When the last command ended.
    last_end: Option<Instant>,
}

/// The turn of a command. Reads wait until it is dropped.
pub(crate) struct CommandTurn<'a> {
    _order: OrderGuard<'a, ()>,
    _count: Counted<'a>,
}

/// Counts a command from the call until it ends, also if the call is dropped
/// while it waits for its turn.
struct Counted<'a>(&'a Priority);

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        let mut state = self.0.lock();
        state.commands -= 1;
        state.last_end = Some(Instant::now());
        if state.commands == 0 {
            self.0.idle.notify_waiters();
        }
    }
}

impl Priority {
    pub(crate) fn new(pause: Duration) -> Self {
        Self {
            pause,
            state: Mutex::new(State {
                commands: 0,
                last_end: None,
            }),
            idle: Notify::new(),
            order: OrderLock::new(()),
        }
    }

    /// Waits for the turn of a command.
    pub(crate) async fn command(&self) -> CommandTurn<'_> {
        self.lock().commands += 1;
        let count = Counted(self);
        CommandTurn {
            _order: self.order.lock().await,
            _count: count,
        }
    }

    /// Waits until a read may start. With `pause` false, the read does not
    /// wait for the pause after the last command.
    pub(crate) async fn read(&self, pause: bool) {
        loop {
            let mut idle = std::pin::pin!(self.idle.notified());
            idle.as_mut().enable();
            match self.read_time(pause) {
                Some(time) if time <= Instant::now() => return,
                Some(time) => tokio::time::sleep_until(time).await,
                None => idle.await,
            }
        }
    }

    /// No command waits or runs, and, with `pause`, the pause after the last
    /// one is over.
    pub(crate) fn read_allowed(&self, pause: bool) -> bool {
        self.read_time(pause)
            .is_some_and(|time| time <= Instant::now())
    }

    /// When a read may start. `None` while a command waits or runs.
    fn read_time(&self, pause: bool) -> Option<Instant> {
        let state = self.lock();
        if state.commands > 0 {
            return None;
        }
        let end = state.last_end.filter(|_| pause);
        Some(end.map_or_else(Instant::now, |end| end + self.pause))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[tokio::test(start_paused = true)]
    async fn a_read_without_commands_starts_at_once() {
        let priority = Priority::new(ms(250));
        let start = Instant::now();
        priority.read(true).await;
        assert_eq!(start.elapsed(), ms(0));
    }

    #[tokio::test(start_paused = true)]
    async fn a_read_waits_for_the_command_and_the_pause() {
        let priority = Arc::new(Priority::new(ms(250)));
        let start = Instant::now();
        let turn = priority.command().await;
        let read = tokio::spawn({
            let priority = priority.clone();
            async move {
                priority.read(true).await;
                Instant::now()
            }
        });
        tokio::time::sleep(ms(100)).await;
        assert!(!priority.read_allowed(true));
        drop(turn);
        assert_eq!(read.await.unwrap().duration_since(start), ms(350));
        assert!(priority.read_allowed(true));
    }

    #[tokio::test(start_paused = true)]
    async fn a_new_command_in_the_pause_makes_the_read_wait_again() {
        let priority = Arc::new(Priority::new(ms(250)));
        let start = Instant::now();
        drop(priority.command().await);
        let read = tokio::spawn({
            let priority = priority.clone();
            async move {
                priority.read(true).await;
                Instant::now()
            }
        });
        tokio::time::sleep(ms(200)).await;
        drop(priority.command().await);
        assert_eq!(read.await.unwrap().duration_since(start), ms(450));
    }

    #[tokio::test(start_paused = true)]
    async fn commands_keep_their_order() {
        let priority = Arc::new(Priority::new(ms(0)));
        let first = priority.command().await;
        let second = tokio::spawn({
            let priority = priority.clone();
            async move {
                let _turn = priority.command().await;
                Instant::now()
            }
        });
        tokio::time::sleep(ms(100)).await;
        let released = Instant::now();
        drop(first);
        assert_eq!(second.await.unwrap(), released);
    }

    #[tokio::test(start_paused = true)]
    async fn a_dropped_command_lets_the_reads_go() {
        let priority = Arc::new(Priority::new(ms(0)));
        let turn = priority.command().await;
        let waiting = tokio::time::timeout(ms(10), priority.command()).await;
        assert!(waiting.is_err());
        drop(turn);
        let start = Instant::now();
        priority.read(true).await;
        assert_eq!(start.elapsed(), ms(0));
    }

    #[tokio::test(start_paused = true)]
    async fn a_read_without_the_pause_waits_only_for_the_command() {
        let priority = Arc::new(Priority::new(ms(250)));
        let start = Instant::now();
        let turn = priority.command().await;
        let read = tokio::spawn({
            let priority = priority.clone();
            async move {
                priority.read(false).await;
                Instant::now()
            }
        });
        tokio::time::sleep(ms(100)).await;
        assert!(!priority.read_allowed(false));
        drop(turn);
        assert_eq!(read.await.unwrap().duration_since(start), ms(100));
        assert!(priority.read_allowed(false) && !priority.read_allowed(true));
    }

    #[tokio::test(start_paused = true)]
    async fn no_pause_allows_a_read_right_after_a_command() {
        let priority = Priority::new(Duration::ZERO);
        drop(priority.command().await);
        assert!(priority.read_allowed(true));
    }
}
