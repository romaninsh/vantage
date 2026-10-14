//! A memory cache that reports what is done to it: how many refresh times
//! were recorded, rows written, rows read and whole-table lists, and, once
//! armed, a single-row write that lands and then stalls, catching a flash
//! mid-stage. Single-row inserts and whole-table lists can also be made to
//! fail outright.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::SystemTime;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use tokio::sync::Notify;
use vantage_core::Result;
use vantage_diorama::{CacheBackend, CacheStatus, CacheTable, MemoryCacheTable};
use vantage_types::Record;

type Rec = Record<CborValue>;

/// One memory table, observed. See the module docs.
#[derive(Default)]
pub struct SpyTable {
    pub inner: MemoryCacheTable,
    /// Refresh times recorded so far.
    pub stamps: AtomicUsize,
    /// Rows written so far, by any insert.
    pub rows_written: AtomicUsize,
    /// Rows read with their status so far, one by one.
    pub status_reads: AtomicUsize,
    /// Whole-table `list_values` reads so far.
    pub lists: AtomicUsize,
    /// When set, the next `insert_value` lands, signals `stalled` and never
    /// returns.
    pub stall_next_insert: AtomicBool,
    pub stalled: Notify,
    /// While set, `insert_value` and `insert_value_with_status` fail without
    /// writing.
    pub fail_inserts: AtomicBool,
    /// While set, `list_values` fails (and still counts as a list).
    pub fail_lists: AtomicBool,
}

impl SpyTable {
    fn check_insert(&self, id: &str) -> Result<()> {
        if self.fail_inserts.load(Ordering::SeqCst) {
            return Err(vantage_core::error!("cache write failed", id = id));
        }
        Ok(())
    }
}

#[async_trait]
impl CacheTable for SpyTable {
    async fn list_values(&self) -> Result<IndexMap<String, Rec>> {
        self.lists.fetch_add(1, Ordering::SeqCst);
        if self.fail_lists.load(Ordering::SeqCst) {
            return Err(vantage_core::error!("cache list failed"));
        }
        self.inner.list_values().await
    }
    async fn get_value(&self, id: &str) -> Result<Option<Rec>> {
        self.inner.get_value(id).await
    }
    async fn insert_value(&self, id: &str, record: &Rec) -> Result<()> {
        self.check_insert(id)?;
        self.rows_written.fetch_add(1, Ordering::SeqCst);
        self.inner.insert_value(id, record).await?;
        if self.stall_next_insert.swap(false, Ordering::SeqCst) {
            self.stalled.notify_one();
            std::future::pending::<()>().await;
        }
        Ok(())
    }
    async fn insert_values(&self, rows: IndexMap<String, Rec>) -> Result<()> {
        self.rows_written.fetch_add(rows.len(), Ordering::SeqCst);
        self.inner.insert_values(rows).await
    }
    async fn delete_value(&self, id: &str) -> Result<()> {
        self.inner.delete_value(id).await
    }
    async fn clear(&self) -> Result<()> {
        self.inner.clear().await
    }
    async fn count(&self) -> Result<i64> {
        self.inner.count().await
    }
    async fn meta_total(&self) -> Result<Option<u64>> {
        self.inner.meta_total().await
    }
    async fn set_meta_total(&self, total: u64) -> Result<()> {
        self.inner.set_meta_total(total).await
    }
    async fn meta_refreshed_at(&self) -> Result<Option<SystemTime>> {
        self.inner.meta_refreshed_at().await
    }
    async fn set_meta_refreshed_at(&self, at: SystemTime) -> Result<()> {
        self.stamps.fetch_add(1, Ordering::SeqCst);
        self.inner.set_meta_refreshed_at(at).await
    }
    async fn meta_copied_at(&self) -> Result<Option<SystemTime>> {
        self.inner.meta_copied_at().await
    }
    async fn set_meta_copied_at(&self, at: SystemTime) -> Result<()> {
        self.inner.set_meta_copied_at(at).await
    }
    async fn get_value_with_status(&self, id: &str) -> Result<Option<(Rec, CacheStatus)>> {
        self.status_reads.fetch_add(1, Ordering::SeqCst);
        self.inner.get_value_with_status(id).await
    }
    async fn insert_value_with_status(
        &self,
        id: &str,
        record: &Rec,
        status: CacheStatus,
    ) -> Result<()> {
        self.check_insert(id)?;
        self.rows_written.fetch_add(1, Ordering::SeqCst);
        self.inner
            .insert_value_with_status(id, record, status)
            .await
    }
    async fn list_values_with_status(&self) -> Result<IndexMap<String, (Rec, CacheStatus)>> {
        self.inner.list_values_with_status().await
    }
}

/// Serves its one table under every name.
pub struct SpyCache(pub Arc<SpyTable>);

#[async_trait]
impl CacheBackend for SpyCache {
    async fn open_table(&self, _name: &str) -> Result<Arc<dyn CacheTable>> {
        Ok(self.0.clone() as Arc<dyn CacheTable>)
    }
    async fn close(&self) {}
    async fn list_tables(&self) -> Result<Vec<String>> {
        Err(vantage_core::error!("SpyCache does not enumerate tables"))
    }
    async fn drop_table(&self, name: &str) -> Result<()> {
        Err(vantage_core::error!(
            "SpyCache does not drop tables",
            table = name
        ))
    }
    fn name(&self) -> &'static str {
        "spy"
    }
}
