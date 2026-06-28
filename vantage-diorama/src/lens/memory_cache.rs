//! In-memory [`CacheBackend`] — one `IndexMap` per Dio, no persistence.
//!
//! The redb backend ([`RedbCache`](super::redb_cache::RedbCache)) is the
//! production path; this mirrors its semantics (including per-row
//! [`CacheStatus`]) without a file, so tests don't need a `TempDir` and
//! two-pass / local-emulation suites see real `Incomplete`/`Complete`
//! round-tripping.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

use super::cache_backend::{CacheBackend, CacheStatus, CacheTable};

/// In-memory cache backend. Each named table is memoized, so repeat
/// `open_table` calls for the same name return the same handle (matching
/// [`RedbCache`](super::redb_cache::RedbCache)).
#[derive(Default)]
pub struct MemoryCache {
    opened: Mutex<IndexMap<String, Arc<MemoryCacheTable>>>,
}

impl MemoryCache {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait]
impl CacheBackend for MemoryCache {
    async fn open_table(&self, name: &str) -> Result<Arc<dyn CacheTable>> {
        let mut opened = self.opened.lock().expect("MemoryCache mutex poisoned");
        if let Some(existing) = opened.get(name) {
            return Ok(existing.clone() as Arc<dyn CacheTable>);
        }
        let table = Arc::new(MemoryCacheTable::default());
        opened.insert(name.to_string(), table.clone());
        Ok(table as Arc<dyn CacheTable>)
    }

    fn name(&self) -> &'static str {
        "memory"
    }
}

#[derive(Default)]
pub struct MemoryCacheTable {
    rows: Mutex<IndexMap<String, (Record<CborValue>, CacheStatus)>>,
}

impl MemoryCacheTable {
    fn lock(&self) -> std::sync::MutexGuard<'_, IndexMap<String, (Record<CborValue>, CacheStatus)>> {
        self.rows.lock().expect("MemoryCacheTable mutex poisoned")
    }

    /// Yield once per operation so this backend is a well-behaved async citizen.
    /// The redb backend suspends at a `spawn_blocking` boundary on every call;
    /// matching that here keeps schedule-sensitive consumers (e.g. event-ordered
    /// reactors) seeing the same interleaving regardless of which cache backs them.
    async fn yield_point() {
        tokio::task::yield_now().await;
    }
}

#[async_trait]
impl CacheTable for MemoryCacheTable {
    async fn list_values(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        Self::yield_point().await;
        Ok(self
            .lock()
            .iter()
            .map(|(id, (rec, _))| (id.clone(), rec.clone()))
            .collect())
    }

    async fn get_value(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        Self::yield_point().await;
        Ok(self.lock().get(id).map(|(rec, _)| rec.clone()))
    }

    async fn insert_value(&self, id: &str, record: &Record<CborValue>) -> Result<()> {
        Self::yield_point().await;
        self.lock()
            .insert(id.to_string(), (record.clone(), CacheStatus::Complete));
        Ok(())
    }

    async fn insert_values(&self, rows: IndexMap<String, Record<CborValue>>) -> Result<()> {
        Self::yield_point().await;
        let mut guard = self.lock();
        for (id, record) in rows {
            guard.insert(id, (record, CacheStatus::Complete));
        }
        Ok(())
    }

    async fn delete_value(&self, id: &str) -> Result<()> {
        Self::yield_point().await;
        self.lock().shift_remove(id);
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        Self::yield_point().await;
        self.lock().clear();
        Ok(())
    }

    async fn count(&self) -> Result<i64> {
        Self::yield_point().await;
        Ok(self.lock().len() as i64)
    }

    async fn insert_value_with_status(
        &self,
        id: &str,
        record: &Record<CborValue>,
        status: CacheStatus,
    ) -> Result<()> {
        Self::yield_point().await;
        self.lock()
            .insert(id.to_string(), (record.clone(), status));
        Ok(())
    }

    async fn get_value_with_status(
        &self,
        id: &str,
    ) -> Result<Option<(Record<CborValue>, CacheStatus)>> {
        Self::yield_point().await;
        Ok(self.lock().get(id).cloned())
    }

    async fn list_values_with_status(
        &self,
    ) -> Result<IndexMap<String, (Record<CborValue>, CacheStatus)>> {
        Self::yield_point().await;
        Ok(self.lock().clone())
    }
}
