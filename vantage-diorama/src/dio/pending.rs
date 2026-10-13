//! Registry of rows with a flash in flight, and of rows whose flash settled
//! since a reconcile began.
//!
//! An optimistic flash stages its value in the cache before the
//! write-through confirms. A reconcile that runs inside that window may
//! carry a master snapshot taken *before* the write — applying it would
//! clobber the staged value and visibly revert the user's edit. Every
//! reconcile-shaped cache writer consults this registry and leaves
//! in-flight rows alone.
//!
//! A flash that settles *after* the snapshot was listed but before the
//! reconcile writes is just as exposed: the snapshot predates it, so it
//! would delete a row the flash just created or revert an edit it just
//! confirmed. A reconcile takes a [`SettleMark`] before it lists and skips
//! every id settled since.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Shared set of in-flight row ids, counted so overlapping flashes on
/// one id keep it protected until the last one settles.
#[derive(Default)]
pub(crate) struct PendingFlashes {
    ids: Mutex<HashMap<String, usize>>,
    settles: Mutex<Settles>,
}

/// The settle clock. Settles are recorded only while a mark is open, so
/// the record stays empty between reconciles.
#[derive(Default)]
struct Settles {
    epoch: u64,
    open_marks: usize,
    settled: HashMap<String, u64>,
}

impl PendingFlashes {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn contains(&self, id: &str) -> bool {
        self.ids.lock().unwrap().contains_key(id)
    }

    /// Mark `id` in flight until the returned guard drops.
    pub(crate) fn begin(self: &Arc<Self>, id: String) -> PendingFlashGuard {
        *self.ids.lock().unwrap().entry(id.clone()).or_insert(0) += 1;
        PendingFlashGuard {
            registry: self.clone(),
            id,
        }
    }

    /// Take a mark now: [`settled_since`](Self::settled_since) then names
    /// every id whose flash settles after this point.
    pub(crate) fn mark(self: &Arc<Self>) -> SettleMark {
        let mut settles = self.settles.lock().unwrap();
        settles.open_marks += 1;
        SettleMark {
            registry: self.clone(),
            epoch: settles.epoch,
        }
    }

    /// Whether `id` is in flight now, or a flash on it settled after `mark`:
    /// either way a snapshot listed at `mark` must leave it alone.
    pub(crate) fn guarded_since(&self, mark: &SettleMark, id: &str) -> bool {
        self.contains(id) || self.settled_since(mark, id)
    }

    fn settled_since(&self, mark: &SettleMark, id: &str) -> bool {
        let settles = self.settles.lock().unwrap();
        settles.settled.get(id).is_some_and(|at| *at > mark.epoch)
    }

    fn settle(&self, id: &str) {
        let mut ids = self.ids.lock().unwrap();
        if let Some(count) = ids.get_mut(id) {
            *count -= 1;
            if *count == 0 {
                ids.remove(id);
            }
        }
        drop(ids);
        let mut settles = self.settles.lock().unwrap();
        settles.epoch += 1;
        if settles.open_marks > 0 {
            let epoch = settles.epoch;
            settles.settled.insert(id.to_string(), epoch);
        }
    }
}

/// RAII marker for one in-flight flash — releases the row on drop, so
/// every exit path of the optimistic flow (commit, rollback, panic
/// unwind) re-opens the row for reconciliation, and records the settle.
pub(crate) struct PendingFlashGuard {
    registry: Arc<PendingFlashes>,
    id: String,
}

impl Drop for PendingFlashGuard {
    fn drop(&mut self) {
        self.registry.settle(&self.id);
    }
}

/// A point on the settle clock, held while a snapshot is listed and
/// reconciled. See [`PendingFlashes::mark`].
pub(crate) struct SettleMark {
    registry: Arc<PendingFlashes>,
    epoch: u64,
}

impl Drop for SettleMark {
    fn drop(&mut self) {
        let mut settles = self.registry.settles.lock().unwrap();
        settles.open_marks -= 1;
        if settles.open_marks == 0 {
            settles.settled.clear();
        }
    }
}
