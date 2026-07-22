//! Registry of rows with a flash in flight.
//!
//! An optimistic flash stages its value in the cache before the
//! write-through confirms. A reconcile that runs inside that window may
//! carry a master snapshot taken *before* the write — applying it would
//! clobber the staged value and visibly revert the user's edit. Every
//! reconcile-shaped cache writer consults this registry and leaves
//! in-flight rows alone; once the flash settles (commit or rollback) the
//! row reconciles normally again.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Shared set of in-flight row ids, counted so overlapping flashes on
/// one id keep it protected until the last one settles.
#[derive(Default)]
pub(crate) struct PendingFlashes {
    ids: Mutex<HashMap<String, usize>>,
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
}

/// RAII marker for one in-flight flash — releases the row on drop, so
/// every exit path of the optimistic flow (commit, rollback, panic
/// unwind) re-opens the row for reconciliation.
pub(crate) struct PendingFlashGuard {
    registry: Arc<PendingFlashes>,
    id: String,
}

impl Drop for PendingFlashGuard {
    fn drop(&mut self) {
        let mut ids = self.registry.ids.lock().unwrap();
        if let Some(count) = ids.get_mut(&self.id) {
            *count -= 1;
            if *count == 0 {
                ids.remove(&self.id);
            }
        }
    }
}
