//! Running totals an engine keeps, read through [`SimEngine::stats`](super::SimEngine::stats).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// A snapshot of an engine's counters.
///
/// Each total is read independently, so a sample is not a consistent
/// snapshot: `ended + errored + live` can briefly exceed `spawned`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SimStats {
    /// Sims running now.
    pub live: usize,
    /// Sims started since the engine started, warm start included.
    pub spawned: u64,
    /// Sims whose script ran to its end, called `done()`, or stopped with the engine.
    pub ended: u64,
    /// Sims ended by a Rhai error (a thrown exception or a budget, depth or
    /// call-level limit) or by a verb panicking.
    pub errored: u64,
    /// Sim write calls: every successful `insert`; an `upsert` that did not leave the
    /// row unchanged; a `patch` or `delete` whose row existed.
    pub writes: u64,
}

#[derive(Default)]
pub(super) struct Counters {
    pub spawned: AtomicU64,
    pub ended: AtomicU64,
    pub errored: AtomicU64,
    /// Shared with the `CountedShell` wrapping every resolved memory Vista,
    /// so writes made through the data vocabulary still count.
    pub writes: Arc<AtomicU64>,
}

impl Counters {
    pub fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub fn read(counter: &AtomicU64) -> u64 {
        counter.load(Ordering::Relaxed)
    }
}
