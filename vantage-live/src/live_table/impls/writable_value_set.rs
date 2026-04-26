//! Write path: every method enqueues a `WriteOp` and awaits the
//! `oneshot` reply. Cache invalidation happens inside the worker, on
//! success only.

use async_trait::async_trait;
use tokio::sync::oneshot;
use tracing::instrument;
use vantage_core::{Result, error};
use vantage_dataset::traits::{ValueSet, WritableValueSet};
use vantage_types::Record;

use crate::live_table::LiveTable;
use crate::live_table::write_op::WriteOp;

/// Map a `mpsc::SendError` into a VantageError. The only way the channel
/// closes is if the worker task panicked or was aborted; surface that as
/// a clear error rather than a generic send failure.
fn closed_queue() -> vantage_core::VantageError {
    error!("vantage-live write queue closed (worker task ended)")
}

/// Map a `oneshot::RecvError` similarly — the worker dropped the reply
/// half before answering, which only happens on panic or shutdown.
fn dropped_reply() -> vantage_core::VantageError {
    error!("vantage-live worker dropped the reply (panic or shutdown)")
}

#[async_trait]
impl WritableValueSet for LiveTable {
    #[instrument(
        target = "vantage_live::write",
        skip(self, record),
        fields(cache_key = %self.cache_key, op = "insert_value", id = %id)
    )]
    async fn insert_value(
        &self,
        id: &<Self as ValueSet>::Id,
        record: &Record<<Self as ValueSet>::Value>,
    ) -> Result<Record<<Self as ValueSet>::Value>> {
        let (tx, rx) = oneshot::channel();
        self.write_queue
            .send(WriteOp::Insert {
                id: id.clone(),
                record: record.clone(),
                reply: tx,
            })
            .await
            .map_err(|_| closed_queue())?;
        rx.await.map_err(|_| dropped_reply())?
    }

    #[instrument(
        target = "vantage_live::write",
        skip(self, record),
        fields(cache_key = %self.cache_key, op = "replace_value", id = %id)
    )]
    async fn replace_value(
        &self,
        id: &<Self as ValueSet>::Id,
        record: &Record<<Self as ValueSet>::Value>,
    ) -> Result<Record<<Self as ValueSet>::Value>> {
        let (tx, rx) = oneshot::channel();
        self.write_queue
            .send(WriteOp::Replace {
                id: id.clone(),
                record: record.clone(),
                reply: tx,
            })
            .await
            .map_err(|_| closed_queue())?;
        rx.await.map_err(|_| dropped_reply())?
    }

    #[instrument(
        target = "vantage_live::write",
        skip(self, partial),
        fields(cache_key = %self.cache_key, op = "patch_value", id = %id)
    )]
    async fn patch_value(
        &self,
        id: &<Self as ValueSet>::Id,
        partial: &Record<<Self as ValueSet>::Value>,
    ) -> Result<Record<<Self as ValueSet>::Value>> {
        let (tx, rx) = oneshot::channel();
        self.write_queue
            .send(WriteOp::Patch {
                id: id.clone(),
                partial: partial.clone(),
                reply: tx,
            })
            .await
            .map_err(|_| closed_queue())?;
        rx.await.map_err(|_| dropped_reply())?
    }

    #[instrument(
        target = "vantage_live::write",
        skip(self),
        fields(cache_key = %self.cache_key, op = "delete", id = %id)
    )]
    async fn delete(&self, id: &<Self as ValueSet>::Id) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.write_queue
            .send(WriteOp::Delete {
                id: id.clone(),
                reply: tx,
            })
            .await
            .map_err(|_| closed_queue())?;
        rx.await.map_err(|_| dropped_reply())?
    }

    #[instrument(
        target = "vantage_live::write",
        skip(self),
        fields(cache_key = %self.cache_key, op = "delete_all")
    )]
    async fn delete_all(&self) -> Result<()> {
        let (tx, rx) = oneshot::channel();
        self.write_queue
            .send(WriteOp::DeleteAll { reply: tx })
            .await
            .map_err(|_| closed_queue())?;
        rx.await.map_err(|_| dropped_reply())?
    }
}
