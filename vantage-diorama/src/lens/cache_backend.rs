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

/// Per-Dio cache handle. Stores `id -> Record<CborValue>` rows.
///
/// Intentionally narrow — no conditions, no sort, no search. The cache
/// is dumb storage; query planning lives on the Dio side. Capability
/// flags on the facade Vista flip according to whether the cache or
/// the master answers a given operation.
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
}
