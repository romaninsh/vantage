//! Running totals an engine keeps, read through [`SimEngine::stats`](super::SimEngine::stats).

use std::sync::atomic::{AtomicU64, Ordering};

use vantage_memory::MemoryStore;

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
    /// Writes that changed a row of the engine's store since the engine
    /// started, warm start included: every successful `insert` (one per
    /// imported row), an `upsert`, `patch` or `replace` that changed its row,
    /// and a `delete` whose row existed. The store's own per-table counters
    /// keep this total, so a write another caller makes to the same store
    /// counts too.
    pub writes: u64,
}

#[derive(Default)]
pub(super) struct Counters {
    pub spawned: AtomicU64,
    pub ended: AtomicU64,
    pub errored: AtomicU64,
    /// The store's write total when the engine started; see [`store_writes`].
    pub writes_before: u64,
}

impl Counters {
    pub fn bump(counter: &AtomicU64) {
        counter.fetch_add(1, Ordering::Relaxed);
    }

    pub fn read(counter: &AtomicU64) -> u64 {
        counter.load(Ordering::Relaxed)
    }
}

/// Every table's write counter in `store`, summed.
pub(super) fn store_writes(store: &MemoryStore) -> u64 {
    store
        .table_names()
        .iter()
        .map(|name| store.table(name).writes())
        .sum()
}
