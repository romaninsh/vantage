//! [`Unstage`]: no staged value outlives a flash that never reached the
//! write queue.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_types::Record;

use super::stage::restore_if_staged;
use crate::dio::pending::PendingFlashGuard;
use crate::dio::{DioEvent, DioInner};

/// Restores a staged row unless its flash reached the write queue. Dropped
/// while armed (the caller was cancelled mid-stage, or staging failed), it
/// puts the pre-image back on the Dio's runtime and publishes
/// [`DioEvent::RecordChanged`] so views re-read the row.
///
/// It holds the flash's pending guard until the restore is done, so no
/// reconcile writes the row in between, and it restores only while the row
/// still holds this flash's staged value: a later flash on the same id that
/// staged over it keeps its own.
pub(super) struct Unstage {
    inner: Option<Arc<DioInner>>,
    id: String,
    pre: Option<Record<CborValue>>,
    staged: Option<Record<CborValue>>,
    pending: Option<PendingFlashGuard>,
}

impl Unstage {
    pub(super) fn new(
        inner: &Arc<DioInner>,
        id: String,
        pre: Option<Record<CborValue>>,
        staged: Option<Record<CborValue>>,
        pending: Option<PendingFlashGuard>,
    ) -> Self {
        Self {
            inner: Some(inner.clone()),
            id,
            pre,
            staged,
            pending,
        }
    }

    /// The flash is queued: the worker settles it from here, and takes the
    /// pending guard with it.
    pub(super) fn disarm(mut self) -> Option<PendingFlashGuard> {
        self.inner = None;
        self.pending.take()
    }
}

impl Drop for Unstage {
    fn drop(&mut self) {
        let Some(inner) = self.inner.take() else {
            return;
        };
        let id = std::mem::take(&mut self.id);
        let (pre, staged, pending) = (self.pre.take(), self.staged.take(), self.pending.take());
        let runtime = inner.lens.runtime.clone();
        runtime.spawn(async move {
            match restore_if_staged(&inner, &id, pre.as_ref(), staged.as_ref()).await {
                Ok(true) => {
                    let _ = inner.event_bus.send(DioEvent::RecordChanged { id });
                }
                Ok(false) => {}
                Err(e) => tracing::error!(
                    target: "vantage_diorama",
                    id = %id,
                    error = %e,
                    "a cancelled flash's staged row was not restored",
                ),
            }
            drop(pending);
        });
    }
}
