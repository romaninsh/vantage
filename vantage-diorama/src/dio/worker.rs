//! The write queue's worker: runs each [`QueuedFlash`] through the Lens's
//! writer, or the master when the Lens has none, settles it, and answers the
//! caller waiting in [`Dio::flash`](crate::Dio::flash).
//!
//! One worker per Dio takes one flash at a time, so the write-throughs of a
//! Dio never overlap and land in the order they were queued.
//!
//! **Drain, not drop**: every queued flash carries a strong
//! `Arc<DioInner>`, so flashes already accepted keep the whole pipeline
//! (master, cache, routes, event bus) alive until they land. Dropping the
//! last external handle never discards queued work. Once the queue is empty
//! and the last keep-alive drops, `DioInner` drops, its sender side
//! disappears, `recv()` returns `None`, and the worker exits cleanly.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use ciborium::Value as CborValue;
use futures::FutureExt as _;
use tokio::sync::{mpsc, oneshot};
use vantage_core::{Context as _, ErrorKind, Result, VantageError, error};
use vantage_dataset::traits::{InsertableValueSet as _, WritableValueSet};
use vantage_types::Record;

use super::flash::settle;
use super::pending::PendingFlashGuard;
use crate::dio::{Dio, DioInner};
use crate::ops::{ChangeFlash, FlashKind, FlashOutcome};

/// One unit of queued work. The temporary reference cycle (`DioInner` →
/// sender → channel buffer → `Arc<DioInner>`) resolves as the worker drains;
/// that transience is exactly the keep-alive guarantee.
pub(crate) struct QueuedFlash {
    pub(crate) flash: ChangeFlash,
    /// The cached row before the flash was staged, which a failed write
    /// restores.
    pub(crate) pre: Option<Record<CborValue>>,
    /// What the flash staged under its id (`None` for a removal, or for an
    /// id-less flash, which stages nothing). A rollback restores `pre` only
    /// while the row still holds it.
    pub(crate) staged: Option<Record<CborValue>>,
    /// Marks the staged row in flight until the flash settles.
    pub(crate) pending: Option<PendingFlashGuard>,
    /// Where the settled outcome goes. The caller may have stopped waiting.
    pub(crate) reply: oneshot::Sender<Result<FlashOutcome>>,
    pub(crate) keep_alive: Arc<DioInner>,
}

/// The per-Dio write worker loop. Owns nothing but the receiver; each
/// message brings its own `Arc<DioInner>`.
pub(crate) async fn write_worker_loop(mut rx: mpsc::Receiver<QueuedFlash>) {
    while let Some(queued) = rx.recv().await {
        let QueuedFlash {
            flash,
            pre,
            staged,
            pending,
            reply,
            keep_alive,
        } = queued;
        let dio = Dio { inner: keep_alive };
        // A panicking route is a failed write: the flash rolls back and the
        // worker carries on with the next one.
        let written = AssertUnwindSafe(run_write_through(&dio, flash.clone()))
            .catch_unwind()
            .await
            .unwrap_or_else(|_| {
                Err(error!(
                    "the write route panicked",
                    table = dio.inner.cache_table_name
                ))
            });
        let settled = settle(
            &dio,
            &flash,
            pending.as_ref(),
            pre.as_ref(),
            staged.as_ref(),
            written,
        )
        .await;
        if let Err(err) = &settled {
            tracing::debug!(error = %err, "Dio flash failed");
        }
        drop(pending);
        if let Some(id) = flash.id() {
            dio.inner.recheck_deferred(id);
        }
        let _ = reply.send(settled);
    }
}

tokio::task_local! {
    /// The Dio whose Lens writer the current task is running, by the address
    /// of its shared state.
    static WRITING_FOR: usize;
}

/// Whether the current task is running `inner`'s Lens writer. A flash on
/// the same Dio from there would queue behind the write it is part of and
/// never settle.
pub(crate) fn writing_for(inner: &Arc<DioInner>) -> bool {
    WRITING_FOR
        .try_with(|writing| *writing == Arc::as_ptr(inner) as usize)
        .unwrap_or(false)
}

/// Run a flash through the Lens's writer, or the default write-to-master
/// path when it has none, inside the flash's
/// [`WriteContext`](vantage_table::write_context::WriteContext). The worker is
/// its own task, so the caller's scope ended at the queue; the flash brought
/// the context across.
async fn run_write_through(dio: &Dio, flash: ChangeFlash) -> Result<FlashOutcome> {
    match flash.context().cloned() {
        Some(context) => context.scope(route(dio, flash)).await,
        None => route(dio, flash).await,
    }
}

async fn route(dio: &Dio, flash: ChangeFlash) -> Result<FlashOutcome> {
    match dio.inner.lens.callbacks.writer.as_ref() {
        Some(writer) => {
            let writing = Arc::as_ptr(&dio.inner) as usize;
            let cx = crate::lens::LoaderCx::new(dio.clone());
            WRITING_FOR
                .scope(writing, async move { writer(cx, flash).await })
                .await
        }
        None => default_write(dio, flash).await,
    }
}

/// The write when the Lens has no writer: apply the flash to `dio.master()`.
/// The cache is not touched here; [`settle`] does that.
async fn default_write(dio: &Dio, flash: ChangeFlash) -> Result<FlashOutcome> {
    let master = dio.master();
    let applied = FlashOutcome::applied();
    let written = match (flash.kind(), flash.id()) {
        // The insert is the one write whose id can move: the record the
        // master stored carries the id it settled on.
        (FlashKind::Insert, Some(id)) => master
            .insert_value(id, flash.patch())
            .await
            .map(|stored| applied.with_record(stored)),
        (FlashKind::Insert, None) => master
            .insert_return_id_value(flash.patch())
            .await
            .map(|id| applied.with_id(id)),
        (FlashKind::Replace, Some(id)) => master
            .replace_value(id, flash.patch())
            .await
            .map(|_| applied),
        (FlashKind::Upsert, Some(id)) => master
            .upsert_value(id, flash.patch())
            .await
            .map(|_| applied),
        (FlashKind::Patch, Some(id)) => {
            master.patch_value(id, flash.patch()).await.map(|_| applied)
        }
        (FlashKind::Delete, Some(id)) => master.delete(id).await.map(|()| applied),
        (FlashKind::Clear, _) => master.delete_all().await.map(|()| applied),
        (kind, None) => Err(error!(
            "flash without an id cannot target a row",
            kind = format!("{kind:?}")
        )),
    };
    // Wrapped, not replaced. The cause has to reach two audiences: a
    // form footer or wizard error line, which shows this string and for
    // which "write failed" alone turns a fixable input problem
    // (a missing `int` the backend named precisely) into a dead end;
    // and `FlashRejection::from_error`, which walks the SOURCE chain for
    // the per-field errors a form highlights. `context` keeps both:
    // `Display` renders "write failed: <driver error>" and the driver's
    // error stays reachable underneath. The wrapper keeps the driver's
    // `Conflict` or `NotFound`: a master that confines writes itself refuses
    // a record outside its set there, and callers branch on the kind. So do
    // a `Forbidden` or `Unsupported` refusal from the master.
    let kind = written.as_ref().err().map(VantageError::kind);
    let wrapped = written.context(error!("write failed"));
    match kind {
        Some(ErrorKind::Conflict) => wrapped.map_err(VantageError::mark_conflict),
        Some(ErrorKind::NotFound) => wrapped.map_err(VantageError::mark_not_found),
        Some(ErrorKind::Forbidden) => wrapped.map_err(VantageError::mark_forbidden),
        Some(ErrorKind::Unsupported) => wrapped.map_err(VantageError::mark_unsupported),
        _ => wrapped,
    }
}
