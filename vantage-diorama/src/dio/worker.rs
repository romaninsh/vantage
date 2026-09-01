//! Write-queue worker — consumes `QueuedFlash`es from `DioInner::write_queue`.
//!
//! For each flash the worker either invokes the lens's `on_flash` route
//! (when registered) or applies the flash directly to `dio.master()`.
//! Route errors are logged via `tracing` and emitted on the event
//! bus as [`DioEvent::WriteFailed`].
//!
//! **Drain, not drop**: every queued flash carries a strong
//! `Arc<DioInner>`, so flashes already accepted keep the whole pipeline
//! (master, cache, routes, event bus) alive until they land — dropping
//! the last external handle never discards queued work. Once the queue
//! is empty and the last keep-alive drops, `DioInner` drops, its sender
//! side disappears, `recv()` returns `None`, and the worker exits
//! cleanly.

use std::sync::Arc;

use ciborium::Value as CborValue;
use tokio::sync::mpsc;
use vantage_core::{Result, error};
use vantage_dataset::traits::WritableValueSet;
use vantage_types::Record;

use crate::dio::{Dio, DioEvent, DioInner};
use crate::ops::{ChangeFlash, FlashKind};

/// One unit of queued work: the flash plus the strong handle that keeps
/// the pipeline alive until it lands. The temporary reference cycle
/// (`DioInner` → sender → channel buffer → `Arc<DioInner>`) resolves as
/// the worker drains — that transience is exactly the keep-alive
/// guarantee.
pub(crate) struct QueuedFlash {
    pub(crate) flash: ChangeFlash,
    pub(crate) keep_alive: Arc<DioInner>,
}

/// The per-Dio write worker loop. Owns nothing but the receiver; each
/// message brings its own `Arc<DioInner>`.
pub(crate) async fn write_worker_loop(mut rx: mpsc::Receiver<QueuedFlash>) {
    while let Some(QueuedFlash { flash, keep_alive }) = rx.recv().await {
        let dio = Dio { inner: keep_alive };
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
///
/// Returns the record a new row was stored as, when the write path
/// produced one. A driver is free to keep the record under an id of its
/// own, so the caller reads the stored record's id rather than assuming
/// the flash's id survived. A routed write reports nothing: the route
/// owns the destination and the caller cannot read its id.
pub(crate) async fn run_write_through(
    dio: &Dio,
    flash: ChangeFlash,
) -> Result<Option<Record<CborValue>>> {
    if let Some(route) = dio.inner.lens.callbacks.on_flash.as_ref() {
        route(dio, flash).await.map(|()| None)
    } else {
        default_write(dio, flash).await
    }
}

/// Default-write path — fires when no `on_flash` route is registered.
/// Applies the flash directly to `dio.master()`. The cache is *not*
/// touched here; the optimistic path stages it, and routes that want a
/// write-through cache update both sides explicitly.
async fn default_write(dio: &Dio, flash: ChangeFlash) -> Result<Option<Record<CborValue>>> {
    let master = dio.master();
    let need_id = || {
        flash
            .id()
            .map(str::to_string)
            .ok_or_else(|| error!("flash without an id cannot target a row"))
    };
    match flash.kind() {
        // The insert is the one write whose id can move: the record the
        // master stored carries the id it settled on.
        FlashKind::Insert => master
            .insert_value(need_id()?, flash.patch())
            .await
            .map(Some),
        FlashKind::Replace => master
            .replace_value(need_id()?, flash.patch())
            .await
            .map(|_| None),
        FlashKind::Patch => master
            .patch_value(need_id()?, flash.patch())
            .await
            .map(|_| None),
        FlashKind::Delete => master.delete(need_id()?).await.map(|()| None),
        FlashKind::Clear => master.delete_all().await.map(|()| None),
    }
    // The cause rides IN the message: this string is what a form footer
    // or a wizard error line shows the user, and "default write failed"
    // alone turns a fixable input problem (a missing `int` the backend
    // named precisely) into a dead end.
    .map_err(|e| error!(format!("write failed: {e}")))
}
