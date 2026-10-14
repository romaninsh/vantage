//! How a pass commits to the cache: in pieces that fit the bus, and row by
//! row when the batched write fails.

use async_trait::async_trait;
use vantage_core::error;

use super::*;
use crate::lens::{CacheBackend, CacheStatus, CacheTable, MemoryCacheTable};

/// A long run is committed in pieces of half the bus, yielding after each,
/// so a subscriber on the same thread reads every event without lagging.
#[tokio::test]
async fn a_long_run_is_committed_in_pieces_that_fit_the_bus() {
    let dio = dio().await;
    let mut bus = dio.subscribe_events();
    let n = EVENT_BUS_CAPACITY * 3;
    let reader = tokio::spawn(async move {
        let mut seen = 0;
        while seen < n {
            match bus.recv().await {
                Ok(_) => seen += 1,
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(seen)
    });
    let changes = (0..n).map(|i| inserted(&format!("{i:04}"), "1"));
    apply_all(&dio, &Admit::All, changes.collect())
        .await
        .unwrap();
    assert_eq!(reader.await.unwrap(), Ok(n));
    assert_eq!(dio.inner.cache.count().await.unwrap(), n as i64);
}

/// A cache whose batched write always fails and whose single-row writes fail
/// for the id `bad`.
struct FlakyTable(MemoryCacheTable);

#[async_trait]
impl CacheTable for FlakyTable {
    async fn list_values(&self) -> Result<IndexMap<String, Rec>> {
        self.0.list_values().await
    }
    async fn get_value(&self, id: &str) -> Result<Option<Rec>> {
        self.0.get_value(id).await
    }
    async fn insert_value(&self, id: &str, record: &Rec) -> Result<()> {
        if id == "bad" {
            return Err(error!("cache write failed", id = id));
        }
        self.0.insert_value(id, record).await
    }
    async fn insert_values(&self, _rows: IndexMap<String, Rec>) -> Result<()> {
        Err(error!("batched cache write failed"))
    }
    async fn delete_value(&self, id: &str) -> Result<()> {
        self.0.delete_value(id).await
    }
    async fn clear(&self) -> Result<()> {
        self.0.clear().await
    }
    async fn count(&self) -> Result<i64> {
        self.0.count().await
    }
    async fn meta_total(&self) -> Result<Option<u64>> {
        self.0.meta_total().await
    }
    async fn set_meta_total(&self, total: u64) -> Result<()> {
        self.0.set_meta_total(total).await
    }
    async fn meta_refreshed_at(&self) -> Result<Option<std::time::SystemTime>> {
        self.0.meta_refreshed_at().await
    }
    async fn set_meta_refreshed_at(&self, at: std::time::SystemTime) -> Result<()> {
        self.0.set_meta_refreshed_at(at).await
    }
    async fn meta_copied_at(&self) -> Result<Option<std::time::SystemTime>> {
        self.0.meta_copied_at().await
    }
    async fn set_meta_copied_at(&self, at: std::time::SystemTime) -> Result<()> {
        self.0.set_meta_copied_at(at).await
    }
    async fn get_value_with_status(&self, id: &str) -> Result<Option<(Rec, CacheStatus)>> {
        self.0.get_value_with_status(id).await
    }
    async fn insert_value_with_status(
        &self,
        id: &str,
        record: &Rec,
        status: CacheStatus,
    ) -> Result<()> {
        if id == "bad" {
            return Err(error!("cache write failed", id = id));
        }
        self.0.insert_value_with_status(id, record, status).await
    }
    async fn list_values_with_status(&self) -> Result<IndexMap<String, (Rec, CacheStatus)>> {
        self.0.list_values_with_status().await
    }
}

/// Serves its one table under every name.
struct FlakyCache(Arc<FlakyTable>);

#[async_trait]
impl CacheBackend for FlakyCache {
    async fn open_table(&self, _name: &str) -> Result<Arc<dyn CacheTable>> {
        Ok(self.0.clone() as Arc<dyn CacheTable>)
    }
    async fn close(&self) {}
    async fn list_tables(&self) -> Result<Vec<String>> {
        Err(error!("FlakyCache does not enumerate tables"))
    }
    async fn drop_table(&self, name: &str) -> Result<()> {
        Err(error!("FlakyCache does not drop tables", table = name))
    }
    fn name(&self) -> &'static str {
        "flaky"
    }
}

/// A failed transaction falls back to row-by-row writes: only the row that
/// can't be written is lost, the changes after it still apply, and the
/// first error is still returned.
#[tokio::test]
async fn a_failed_commit_loses_only_the_rows_that_fail() {
    let table = Arc::new(FlakyTable(MemoryCacheTable::default()));
    table.0.insert_value("old", &row("1")).await.unwrap();
    let dio = dio_over(Cache::backend(Arc::new(FlakyCache(table)))).await;
    let mut bus = dio.subscribe_events();
    let changes = vec![
        inserted("a", "1"),
        inserted("bad", "1"),
        VistaChange::Deleted { id: "old".into() },
        inserted("c", "1"),
    ];
    let result = apply_all(&dio, &Admit::All, changes).await;
    assert!(result.is_err());

    let cache = &dio.inner.cache;
    assert_eq!(cache.get_value("a").await.unwrap(), Some(row("1")));
    assert_eq!(cache.get_value("bad").await.unwrap(), None);
    assert_eq!(cache.get_value("old").await.unwrap(), None);
    assert_eq!(cache.get_value("c").await.unwrap(), Some(row("1")));
    assert_eq!(drain(&mut bus), ["inserted a", "removed old", "inserted c"]);
}
