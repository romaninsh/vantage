//! Write-queue worker — consumes [`WriteOp`]s from `DioInner::write_queue`.
//!
//! For each op the worker either invokes the lens's `on_write` callback
//! (when registered) or applies the op directly to `dio.master()`.
//! Callback errors are logged via `tracing` and emitted on the event
//! bus as [`DioEvent::WriteFailed`]; the worker keeps running until
//! the last external Dio handle drops (at which point the sender side
//! of the channel disappears and `recv()` returns `None`).

use std::sync::Weak;

use tokio::sync::mpsc;
use vantage_core::{Result, error};
use vantage_dataset::traits::WritableValueSet;

use crate::dio::{Dio, DioEvent, DioInner};
use crate::ops::WriteOp;

/// The per-Dio write worker loop. Only holds a `Weak<DioInner>` so the
/// cycle (Dio → channel → worker → Dio) breaks when external handles
/// drop. Each iteration upgrades the Weak to build a real Dio for the
/// callback; if upgrade fails (very narrow race: op sent right before
/// the last external Dio dropped), the op is discarded.
pub(crate) async fn write_worker_loop(inner: Weak<DioInner>, mut rx: mpsc::Receiver<WriteOp>) {
    while let Some(op) = rx.recv().await {
        let Some(strong) = inner.upgrade() else {
            return;
        };
        let dio = Dio { inner: strong };
        let id_for_event = op.id().map(str::to_string);
        let outcome = dispatch(&dio, op).await;
        if let Err(err) = outcome {
            tracing::error!(error = %err, "Dio write op failed");
            let _ = dio.inner.event_bus.send(DioEvent::WriteFailed {
                id: id_for_event,
                error: err.to_string(),
            });
        }
    }
}

async fn dispatch(dio: &Dio, op: WriteOp) -> Result<()> {
    if let Some(cb) = dio.inner.lens.callbacks.on_write.as_ref() {
        cb(dio, op).await
    } else {
        default_write(dio, op).await
    }
}

/// Default-write path — fires when no `on_write` callback is registered.
/// Applies the op directly to `dio.master()`. The cache is *not* touched;
/// users who want write-through cache write an `on_write` callback that
/// updates both sides explicitly.
async fn default_write(dio: &Dio, op: WriteOp) -> Result<()> {
    let master = dio.master();
    match op {
        WriteOp::Insert { id, record } => master.insert_value(id, &record).await.map(|_| ()),
        WriteOp::Replace { id, record } => master.replace_value(id, &record).await.map(|_| ()),
        WriteOp::Patch { id, partial } => master.patch_value(id, &partial).await.map(|_| ()),
        WriteOp::Delete { id } => master.delete(id).await,
        WriteOp::DeleteAll => master.delete_all().await,
    }
    .map_err(|e| error!("default write failed", detail = e.to_string()))
}
