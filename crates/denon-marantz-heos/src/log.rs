//! Logging helpers.

use std::sync::atomic::{AtomicU32, Ordering};

use tracing::Span;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// Span for one client. Its connections and background task log in it. `id`
/// is unique in this process.
pub(crate) fn session(host: &str) -> Span {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    tracing::error_span!(target: "denon_marantz_heos", "heos", id, host = %host)
}
