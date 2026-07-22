use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::{Column, Reference, TableShell, Vista, VistaCapabilities};

use crate::dio::shell::DioShell;
use crate::ops::ChangeFlash;

#[async_trait]
impl TableShell for DioShell {
    // ---- Schema forwarding to master ----------------------------------------

    fn columns(&self) -> &IndexMap<String, Column> {
        &self.columns
    }

    fn references(&self) -> &IndexMap<String, Reference> {
        &self.references
    }

    fn id_column(&self) -> Option<&str> {
        self.id_column.as_deref()
    }

    // ---- Reads — cache-first; bounded reads hydrate ---------------------------
    //
    // When the Dio carries augmentations, a BOUNDED facade read
    // (`get_value`, `fetch_window`) runs the detail pass for every
    // returned row that still has an augment gap — the read blocks until
    // its rows are fully hydrated (and cached, so the cost is paid once).
    // `list_values` deliberately stays cheap: a listing is the fast spine,
    // and hydrating an entire set through it means one innocent-looking
    // call downloading everything. Ask for a window when you want details.

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
        let Some(row) = self.dio.cache.get_value(id).await? else {
            return Ok(None);
        };
        let mut rows = IndexMap::from([(id.clone(), row)]);
        self.hydrate(&mut rows).await?;
        Ok(rows.shift_remove(id))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let rows = self.dio.cache.list_values().await?;
        let Some((id, row)) = rows.into_iter().next() else {
            return Ok(None);
        };
        let mut one = IndexMap::from([(id.clone(), row)]);
        self.hydrate(&mut one).await?;
        Ok(one.into_iter().next())
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        self.dio.cache.count().await
    }

    async fn fetch_window(
        &self,
        _vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        let all = self.dio.cache.list_values().await?;
        let mut rows: IndexMap<String, Record<CborValue>> =
            all.into_iter().skip(offset).take(limit).collect();
        self.hydrate(&mut rows).await?;
        Ok(rows.into_iter().collect())
    }

    // ---- Writes — enqueue + return synthesized record ------------------------
    //
    // Writes are fire-and-forget: the queue accepts the op and returns
    // immediately. Failures land on the event bus as
    // `DioEvent::WriteFailed`. The synthesized record echoes the input
    // (with the id injected) — callers that need authoritative
    // server-side data should refetch via `get_value` after the write
    // completes (an `on_flash` route typically updates the cache too).

    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.enqueue(ChangeFlash::insert(id.clone(), record.clone()))
            .await?;
        Ok(with_injected_id(record, id))
    }

    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.enqueue(ChangeFlash::replace(id.clone(), record.clone()))
            .await?;
        Ok(with_injected_id(record, id))
    }

    async fn patch_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.enqueue(ChangeFlash::new(
            crate::ops::FlashKind::Patch,
            Some(id.clone()),
            partial.clone(),
        ))
        .await?;
        Ok(with_injected_id(partial, id))
    }

    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        self.enqueue(ChangeFlash::delete(id.clone())).await
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        self.enqueue(ChangeFlash::clear()).await
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
    /// Facade reads hydrate what they return: any row still missing its
    /// augment columns runs the Dio's detail pass before the read
    /// resolves. No augmentations configured → no-op.
    async fn hydrate(&self, rows: &mut IndexMap<String, Record<CborValue>>) -> Result<()> {
        if !self.dio.has_dio_augment() {
            return Ok(());
        }
        let dio = crate::Dio {
            inner: self.dio.clone(),
        };
        crate::dio::augment_passes::hydrate_gaps(&dio, rows).await
    }

    async fn enqueue(&self, flash: ChangeFlash) -> Result<()> {
        // The queued flash carries its own keep-alive: even if every
        // external handle drops right after this returns, the pipeline
        // stays alive until the write lands.
        self.dio
            .write_queue
            .send(crate::dio::worker::QueuedFlash {
                flash,
                keep_alive: self.dio.clone(),
            })
            .await
            .map_err(|e| error!("Dio write queue closed", detail = e.to_string()))
    }
}

fn with_injected_id(record: &Record<CborValue>, id: &str) -> Record<CborValue> {
    let mut out = record.clone();
    out.insert("id".to_string(), CborValue::Text(id.to_string()));
    out
}
