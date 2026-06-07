use std::sync::Arc;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

/// Storage backing a [`Lens`](super::Lens).
///
/// Each Dio under a Lens claims one named table within the backend.
/// `open_table(name)` returns the per-Dio handle that DioShell reads
/// from and that `on_start` callbacks write to.
#[async_trait]
pub trait CacheBackend: Send + Sync + 'static {
    /// Open (or create) the named cache table. Backends are free to
    /// memoize so repeat calls for the same name return the same Arc.
    async fn open_table(&self, name: &str) -> Result<Arc<dyn CacheTable>>;

    /// Short human label for diagnostics (`"redb"`, `"memory"`).
    fn name(&self) -> &'static str {
        "unknown"
    }
}

/// Persisted completeness of a cached record. Two-pass loading writes
/// [`Incomplete`](CacheStatus::Incomplete) rows from the list pass (id +
/// cheap columns) and flips them to [`Complete`](CacheStatus::Complete)
/// once the detail pass hydrates them. Persisting this lets hydration
/// resume across restarts and skip records that are already complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CacheStatus {
    /// Fully hydrated — no further detail fetch needed.
    #[default]
    Complete,
    /// Only partially loaded (list pass) — awaiting detail hydration.
    Incomplete,
}

/// Per-Dio cache handle. Stores `id -> (status, Record<CborValue>)` rows.
///
/// Intentionally narrow — no conditions, no sort, no search. The cache
/// is dumb storage; query planning lives on the Dio side. Capability
/// flags on the facade Vista flip according to whether the cache or
/// the master answers a given operation.
///
/// The status-agnostic methods (`get_value`, `insert_value`, …) treat
/// every record as [`CacheStatus::Complete`]; two-pass callers use the
/// `*_with_status` variants to read/write completeness.
#[async_trait]
pub trait CacheTable: Send + Sync + 'static {
    async fn list_values(&self) -> Result<IndexMap<String, Record<CborValue>>>;

    async fn get_value(&self, id: &str) -> Result<Option<Record<CborValue>>>;

    async fn insert_value(&self, id: &str, record: &Record<CborValue>) -> Result<()>;

    /// Bulk write — typical `on_start` shape. Implementations may
    /// commit in a single transaction.
    async fn insert_values(&self, rows: IndexMap<String, Record<CborValue>>) -> Result<()>;

    async fn delete_value(&self, id: &str) -> Result<()>;

    async fn clear(&self) -> Result<()>;

    async fn count(&self) -> Result<i64>;

    /// Read a record together with its persisted [`CacheStatus`]. The
    /// default treats any stored record as `Complete`; persisting backends
    /// override this.
    async fn get_value_with_status(
        &self,
        id: &str,
    ) -> Result<Option<(Record<CborValue>, CacheStatus)>> {
        Ok(self
            .get_value(id)
            .await?
            .map(|r| (r, CacheStatus::Complete)))
    }

    /// Write a record with an explicit [`CacheStatus`]. The default drops
    /// the status (writes a plain record); persisting backends override.
    async fn insert_value_with_status(
        &self,
        id: &str,
        record: &Record<CborValue>,
        _status: CacheStatus,
    ) -> Result<()> {
        self.insert_value(id, record).await
    }

    /// List records together with their persisted statuses. The default
    /// reports every record as `Complete`.
    async fn list_values_with_status(
        &self,
    ) -> Result<IndexMap<String, (Record<CborValue>, CacheStatus)>> {
        Ok(self
            .list_values()
            .await?
            .into_iter()
            .map(|(id, r)| (id, (r, CacheStatus::Complete)))
            .collect())
    }
}
