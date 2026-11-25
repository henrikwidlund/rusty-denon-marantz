//! Logging helpers.

use std::sync::atomic::{AtomicU32, Ordering};

use tracing::Span;

static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// Span for one receiver. Its requests, connections, reads and background
/// tasks log in it. `id` is unique in this process.
pub(crate) fn session(host: &str) -> Span {
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    tracing::error_span!(target: "denon_marantz", "avr", id, host = %host)
}
