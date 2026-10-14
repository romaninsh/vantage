//! Coalesced reseeds for a refined two-pass view.
//!
//! A refined view re-derives its whole visible map when a row's values move,
//! since the move may change what it admits and where the row sorts. A sweep
//! moves every listed row, and a reseed reads every listed row, so one reseed
//! per hydrated row is quadratic in the index. A moved row marks the rows
//! dirty instead, and one task reseeds once per batch.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::time::Duration;

use tokio::sync::Notify;

use super::super::state::SharedRows;
use super::refine::reseed_filtered;

/// Whether the rows wait for a reseed, and the wake-up for the task that
/// runs it.
#[derive(Default)]
pub(crate) struct Coalescer {
    dirty: AtomicBool,
    notify: Notify,
}

impl Coalescer {
    /// Ask for a reseed. Only the first ask since the last reseed wakes the
    /// task; the rest join its batch.
    pub(crate) fn mark_dirty(&self) {
        if !self.dirty.swap(true, Ordering::SeqCst) {
            self.notify.notify_one();
        }
    }
}

/// Wait for an ask, let `debounce` gather the rest of the batch, then reseed
/// and bump once. Holds the rows weakly while it waits.
pub(crate) async fn reseed_loop(
    rows: Weak<SharedRows>,
    coalescer: Arc<Coalescer>,
    debounce: Duration,
) {
    loop {
        coalescer.notify.notified().await;
        tokio::time::sleep(debounce).await;
        let Some(state) = rows.upgrade() else {
            return;
        };
        // Cleared before the reseed reads the cache: a row that moves while
        // it runs asks again, and the permit it leaves wakes the next batch.
        coalescer.dirty.store(false, Ordering::SeqCst);
        reseed_filtered(&state).await;
        state.bump_generation();
    }
}
