use std::sync::Arc;
use std::time::SystemTime;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

/// Storage backing a [`Lens`](super::Lens).
///
/// Each Dio under a Lens claims one named table within the backend.
/// `open_table(name)` returns the per-Dio handle that DioShell reads
/// from and that loaders write to.
#[async_trait]
pub trait CacheBackend: Send + Sync + 'static {
    /// Open (or create) the named cache table. Backends are free to
    /// memoize so repeat calls for the same name return the same Arc.
    async fn open_table(&self, name: &str) -> Result<Arc<dyn CacheTable>>;

    /// Close the backend cleanly, before the process exits.
    ///
    /// Some stores record whether they were shut down properly and, finding
    /// they weren't, rebuild from a full-file scan on the next open. That scan
    /// is paid on every subsequent launch and grows with the file, so a store
    /// that is only ever closed by dropping — at an exit that may never run
    /// destructors — degrades startup permanently.
    ///
    /// Idempotent and safe to call while readers still hold tables. A store
    /// with no such state implements it as a no-op.
    async fn close(&self);

    /// Every table currently present in the backend.
    ///
    /// The counterpart to [`drop_table`](Self::drop_table): a consumer that
    /// encodes query variants (or a config epoch) into the *table* name needs
    /// to enumerate what is there to know what is stale. A backend that
    /// cannot enumerate returns an error.
    async fn list_tables(&self) -> Result<Vec<String>>;

    /// Remove a table and its rows entirely.
    ///
    /// Distinct from [`CacheTable::clear`], which empties a table that stays
    /// open: this reclaims the name, and the table's meta goes with it.
    /// Dropping a table that does not exist is not an error.
    async fn drop_table(&self, name: &str) -> Result<()>;

    /// Short human label for diagnostics (`"redb"`, `"memory"`).
    fn name(&self) -> &'static str;
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
/// The status-agnostic writes (`insert_value`, `insert_values`) store
/// every record as [`CacheStatus::Complete`]; two-pass callers use the
/// `*_with_status` variants to read/write completeness. Every method is
/// required: a backend that dropped the status would silently break
/// two-pass resume.
#[async_trait]
pub trait CacheTable: Send + Sync + 'static {
    async fn list_values(&self) -> Result<IndexMap<String, Record<CborValue>>>;

    async fn get_value(&self, id: &str) -> Result<Option<Record<CborValue>>>;

    async fn insert_value(&self, id: &str, record: &Record<CborValue>) -> Result<()>;

    /// Bulk write — typical eager-start shape. Implementations may
    /// commit in a single transaction.
    async fn insert_values(&self, rows: IndexMap<String, Record<CborValue>>) -> Result<()>;

    async fn delete_value(&self, id: &str) -> Result<()>;

    /// Remove every row. Also forgets [`meta_refreshed_at`](Self::meta_refreshed_at)
    /// and [`meta_copied_at`](Self::meta_copied_at), so an emptied table never
    /// looks fresh or whole; the stated total stays.
    async fn clear(&self) -> Result<()>;

    async fn count(&self) -> Result<i64>;

    /// The master set's grand total as last STATED by a fetch (a counted
    /// window response) — persisted so a reopened
    /// paged view can size itself to its final geometry before any fetch
    /// runs, instead of growing in a visible step when the first counted
    /// response lands. `None` = never learned.
    async fn meta_total(&self) -> Result<Option<u64>>;

    /// Persist the stated grand total — see [`Self::meta_total`]. Callers
    /// must not write totals of a *narrowed* fetch (an active quicksearch):
    /// the meta describes the table the next open will show.
    async fn set_meta_total(&self, total: u64) -> Result<()>;

    /// When the table's contents were last refreshed from the master, as
    /// recorded by [`set_meta_refreshed_at`](Self::set_meta_refreshed_at).
    /// `None` = never recorded. Persisted so a refresher's maximum age holds
    /// across restarts.
    async fn meta_refreshed_at(&self) -> Result<Option<SystemTime>>;

    /// Record when the table's contents were refreshed — see
    /// [`Self::meta_refreshed_at`]. A backend may refuse a time it can't
    /// store with an error; redb refuses one before the Unix epoch.
    async fn set_meta_refreshed_at(&self, at: SystemTime) -> Result<()>;

    /// When the table last became a whole copy of the master's set, as
    /// recorded by [`set_meta_copied_at`](Self::set_meta_copied_at). `None`
    /// = never. Unlike [`meta_refreshed_at`](Self::meta_refreshed_at), which
    /// any refresh records (a paged or load-on-miss table included), this is
    /// written only after every row of the set was written and every row
    /// outside it dropped, so a reader may serve the whole set from the
    /// table. [`clear`](Self::clear) forgets it.
    async fn meta_copied_at(&self) -> Result<Option<SystemTime>>;

    /// Record that the table is a whole copy — see [`Self::meta_copied_at`].
    /// A backend may refuse a time it can't store, as for
    /// [`set_meta_refreshed_at`](Self::set_meta_refreshed_at).
    async fn set_meta_copied_at(&self, at: SystemTime) -> Result<()>;

    /// Read a record together with its persisted [`CacheStatus`].
    async fn get_value_with_status(
        &self,
        id: &str,
    ) -> Result<Option<(Record<CborValue>, CacheStatus)>>;

    /// Write a record with an explicit [`CacheStatus`].
    async fn insert_value_with_status(
        &self,
        id: &str,
        record: &Record<CborValue>,
        status: CacheStatus,
    ) -> Result<()>;

    /// List records together with their persisted statuses.
    async fn list_values_with_status(
        &self,
    ) -> Result<IndexMap<String, (Record<CborValue>, CacheStatus)>>;
}
