//! Write-queue worker — consumes [`ChangeFlash`]es from `DioInner::write_queue`.
//!
//! For each flash the worker either invokes the lens's `on_flash` route
//! (when registered) or applies the flash directly to `dio.master()`.
//! Route errors are logged via `tracing` and emitted on the event
//! bus as [`DioEvent::WriteFailed`]; the worker keeps running until
//! the last external Dio handle drops (at which point the sender side
//! of the channel disappears and `recv()` returns `None`).

use std::sync::Weak;

use tokio::sync::mpsc;
use vantage_core::{Result, error};
use vantage_dataset::traits::WritableValueSet;

use crate::dio::{Dio, DioEvent, DioInner};
use crate::ops::{ChangeFlash, FlashKind};

/// The per-Dio write worker loop. Only holds a `Weak<DioInner>` so the
/// cycle (Dio → channel → worker → Dio) breaks when external handles
/// drop. Each iteration upgrades the Weak to build a real Dio for the
/// callback; if upgrade fails (very narrow race: flash sent right before
/// the last external Dio dropped), the flash is discarded.
pub(crate) async fn write_worker_loop(inner: Weak<DioInner>, mut rx: mpsc::Receiver<ChangeFlash>) {
    while let Some(flash) = rx.recv().await {
        let Some(strong) = inner.upgrade() else {
            return;
        };
        let dio = Dio { inner: strong };
        let id_for_event = flash.id().map(str::to_string);
        let outcome = run_write_through(&dio, flash).await;
        if let Err(err) = outcome {
            tracing::error!(error = %err, "Dio flash failed");
            let _ = dio.inner.event_bus.send(DioEvent::WriteFailed {
                id: id_for_event,
                error: err.to_string(),
            });
        }
    }
}

/// Run a flash through the lens's `on_flash` route, or the default
/// write-to-master path when none is registered. Shared by the
/// fire-and-forget queue worker and the optimistic path
/// ([`Dio::flash`](crate::Dio::flash)).
pub(crate) async fn run_write_through(dio: &Dio, flash: ChangeFlash) -> Result<()> {
    if let Some(route) = dio.inner.lens.callbacks.on_flash.as_ref() {
        route(dio, flash).await
    } else {
        default_write(dio, flash).await
    }
}

/// Default-write path — fires when no `on_flash` route is registered.
/// Applies the flash directly to `dio.master()`. The cache is *not*
/// touched here; the optimistic path stages it, and routes that want a
/// write-through cache update both sides explicitly.
async fn default_write(dio: &Dio, flash: ChangeFlash) -> Result<()> {
    let master = dio.master();
    let need_id = || {
        flash
            .id()
            .map(str::to_string)
            .ok_or_else(|| error!("flash without an id cannot target a row"))
    };
    match flash.kind() {
        FlashKind::Insert => master
            .insert_value(need_id()?, flash.patch())
            .await
            .map(|_| ()),
        FlashKind::Replace => master
            .replace_value(need_id()?, flash.patch())
            .await
            .map(|_| ()),
        FlashKind::Patch => master
            .patch_value(need_id()?, flash.patch())
            .await
            .map(|_| ()),
        FlashKind::Delete => master.delete(need_id()?).await,
        FlashKind::Clear => master.delete_all().await,
    }
    .map_err(|e| error!("default write failed", detail = e.to_string()))
}
