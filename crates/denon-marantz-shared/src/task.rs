//! Request handling of the background tasks.

use std::future::Future;

use tokio::sync::{mpsc, watch};

/// A request that can be refused when there is no connection.
pub trait Refuse {
    /// Tells the caller that the request was not done.
    fn refuse(self);
}

/// Tells the callers of waiting requests that nothing was sent.
pub fn drop_requests<R: Refuse>(requests: &mut mpsc::Receiver<R>) {
    while let Ok(request) = requests.try_recv() {
        request.refuse();
    }
}

/// Runs `work`. Requests that come in meanwhile are refused, because there is
/// no connection to send them on.
pub async fn refusing_requests<R: Refuse, T>(
    requests: &mut mpsc::Receiver<R>,
    work: impl Future<Output = T>,
) -> T {
    tokio::pin!(work);
    loop {
        tokio::select! {
            output = &mut work => return output,
            Some(request) = requests.recv() => request.refuse(),
        }
    }
}

/// Runs `work`. `None` if the task is stopped first.
pub async fn until_stopped<T>(
    shutdown: &mut watch::Receiver<bool>,
    work: impl Future<Output = T>,
) -> Option<T> {
    tokio::select! {
        biased;
        _ = shutdown.changed() => None,
        output = work => Some(output),
    }
}
