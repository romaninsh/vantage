//! Write-queue worker task.
//!
//! Drains `WriteOp`s from the channel, applies each to the master (or the
//! custom write target), and invalidates the cache for `cache_key` on
//! success. Errors are reported back to the caller via the per-op
//! `oneshot` reply; the cache is left untouched on failure.

use std::sync::Arc;

use tokio::sync::mpsc;
use tracing::{debug, instrument, warn, Instrument as _};
use vantage_dataset::traits::WritableValueSet;
use vantage_table::any::AnyTable;

use crate::cache::Cache;

use super::write_op::WriteOp;

pub(super) fn spawn(
    mut rx: mpsc::Receiver<WriteOp>,
    master: AnyTable,
    custom_write_target: Option<AnyTable>,
    cache_key: String,
    cache: Arc<dyn Cache>,
) {
    // Propagate the caller's tracing span across the spawn boundary so
    // worker errors stitch into the same trace as the originating write.
    tokio::spawn(
        async move {
            while let Some(op) = rx.recv().await {
                handle(&master, &custom_write_target, &cache_key, &cache, op).await;
            }
            debug!(target: "vantage_live::worker", cache_key = %cache_key, "write-queue worker shutting down");
        }
        .in_current_span(),
    );
}

#[instrument(
    target = "vantage_live::worker",
    skip_all,
    fields(cache_key = %cache_key, op = op.kind())
)]
async fn handle(
    master: &AnyTable,
    custom_write_target: &Option<AnyTable>,
    cache_key: &str,
    cache: &Arc<dyn Cache>,
    op: WriteOp,
) {
    // Pick destination — defaults to master, overridable per LiveTable.
    let target = custom_write_target.as_ref().unwrap_or(master);

    let result_invalidates: bool = match op {
        WriteOp::Insert { id, record, reply } => {
            let r = target.insert_value(&id, &record).await;
            let ok = r.is_ok();
            let _ = reply.send(r);
            ok
        }
        WriteOp::Replace { id, record, reply } => {
            let r = target.replace_value(&id, &record).await;
            let ok = r.is_ok();
            let _ = reply.send(r);
            ok
        }
        WriteOp::Patch { id, partial, reply } => {
            let r = target.patch_value(&id, &partial).await;
            let ok = r.is_ok();
            let _ = reply.send(r);
            ok
        }
        WriteOp::Delete { id, reply } => {
            let r = target.delete(&id).await;
            let ok = r.is_ok();
            let _ = reply.send(r);
            ok
        }
        WriteOp::DeleteAll { reply } => {
            let r = target.delete_all().await;
            let ok = r.is_ok();
            let _ = reply.send(r);
            ok
        }
    };

    if result_invalidates {
        if let Err(e) = cache.invalidate_prefix(cache_key).await {
            warn!(
                target: "vantage_live::worker",
                error = %e,
                "cache invalidation failed after successful write"
            );
        }
    } else {
        // Failed writes leave the cache alone — surface a debug line so
        // a debug-level trace shows the path was taken.
        debug!(target: "vantage_live::worker", "write failed; cache untouched");
    }
}

impl WriteOp {
    /// Short identifier used in tracing fields. Matches the dataset trait
    /// method names so logs cross-reference cleanly.
    fn kind(&self) -> &'static str {
        match self {
            WriteOp::Insert { .. } => "insert_value",
            WriteOp::Replace { .. } => "replace_value",
            WriteOp::Patch { .. } => "patch_value",
            WriteOp::Delete { .. } => "delete",
            WriteOp::DeleteAll { .. } => "delete_all",
        }
    }
}
