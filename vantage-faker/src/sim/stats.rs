//! Running totals an engine keeps, read through [`SimEngine::stats`](super::SimEngine::stats).

use std::sync::atomic::{AtomicU64, Ordering};

/// A snapshot of an engine's counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SimStats {
    /// Sims running now.
    pub live: usize,
    /// Sims started since the engine started, warm start included.
    pub spawned: u64,
    /// Sims whose script ran to its end, called `done()`, or stopped with the engine.
    pub ended: u64,
    /// Sims ended by a Rhai error: a thrown exception or a budget, depth or call-level limit.
    pub errored: u64,
    /// `insert`, `set`, `patch` and `delete` calls that changed a row.
    pub writes: u64,
}

#[derive(Default)]
pub(super) struct Counters {
    pub spawned: AtomicU64,
    pub ended: AtomicU64,
    pub errored: AtomicU64,
    pub writes: AtomicU64,
}

impl Counters {
    pub fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub fn read(counter: &AtomicU64) -> u64 {
        counter.load(Ordering::Relaxed)
    }
}
