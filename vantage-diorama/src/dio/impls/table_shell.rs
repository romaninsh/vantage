use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::{Column, Reference, TableShell, Vista, VistaCapabilities};

use crate::dio::shell::DioShell;
use crate::ops::WriteOp;

#[async_trait]
impl TableShell for DioShell {
    // ---- Schema forwarding to master ----------------------------------------

    fn columns(&self) -> &IndexMap<String, Column> {
        self.dio.master.source.columns()
    }

    fn references(&self) -> &IndexMap<String, Reference> {
        self.dio.master.source.references()
    }

    fn id_column(&self) -> Option<&str> {
        self.dio.master.source.id_column()
    }

    // ---- Reads — cache-first --------------------------------------------------

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.dio.cache.list_values().await
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        self.dio.cache.get_value(id).await
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let rows = self.dio.cache.list_values().await?;
        Ok(rows.into_iter().next())
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        self.dio.cache.count().await
    }

    // ---- Writes — enqueue + return synthesized record ------------------------
    //
    // Writes are fire-and-forget: the queue accepts the op and returns
    // immediately. Failures land on the event bus as
    // `DioEvent::WriteFailed`. The synthesized record echoes the input
    // (with the id injected) — callers that need authoritative
    // server-side data should refetch via `get_value` after the write
    // completes (`on_write` typically updates the cache too).

    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.enqueue(WriteOp::Insert {
            id: id.clone(),
            record: record.clone(),
        })
        .await?;
        Ok(with_injected_id(record, id))
    }

    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.enqueue(WriteOp::Replace {
            id: id.clone(),
            record: record.clone(),
        })
        .await?;
        Ok(with_injected_id(record, id))
    }

    async fn patch_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.enqueue(WriteOp::Patch {
            id: id.clone(),
            partial: partial.clone(),
        })
        .await?;
        Ok(with_injected_id(partial, id))
    }

    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        self.enqueue(WriteOp::Delete { id: id.clone() }).await
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        self.enqueue(WriteOp::DeleteAll).await
    }

    // ---- Capability + identity ------------------------------------------------

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "dio"
    }
}

impl DioShell {
    async fn enqueue(&self, op: WriteOp) -> Result<()> {
        self.dio
            .write_queue
            .send(op)
            .await
            .map_err(|e| error!("Dio write queue closed", detail = e.to_string()))
    }
}

fn with_injected_id(record: &Record<CborValue>, id: &str) -> Record<CborValue> {
    let mut out = record.clone();
    out.insert("id".to_string(), CborValue::Text(id.to_string()));
    out
}
