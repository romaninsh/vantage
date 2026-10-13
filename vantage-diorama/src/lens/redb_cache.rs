//! Redb-backed [`CacheBackend`] — one file per Lens, one table per Dio.
//!
//! Rows are stored as `id (str) -> status tag + cbor body (bytes)`; see
//! `status.rs` for the layout. Encoding uses [`ciborium`] directly; the cache
//! doesn't know anything about the master Vista's schema, just opaque records
//! keyed by id. Each table's meta (stated total, last refresh) lives in a
//! sibling table; see `meta.rs`.
//!
//! Redb operations are synchronous, so the async methods wrap each unit
//! of work in `tokio::task::spawn_blocking`. The cache shares the
//! `Arc<redb::Database>` across opened tables.

mod meta;
mod status;
mod table;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use indexmap::IndexMap;
use redb::{Database, TableDefinition, TableHandle};
use vantage_core::{Result, error};

use super::cache_backend::{CacheBackend, CacheTable};

pub use table::RedbCacheTable;

/// Every redb table this cache writes: string keys, byte values.
fn table_def(name: &str) -> TableDefinition<'_, &'static str, &'static [u8]> {
    TableDefinition::new(name)
}

/// Run blocking redb work off the async runtime.
async fn blocking<T, F>(work: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| error!("blocking task panicked", detail = e.to_string()))?
}

/// `RedbCache` opens (or creates) a redb database at the configured
/// path. Each Dio under the owning Lens claims a named table within it.
pub struct RedbCache {
    db: Arc<Database>,
    path: PathBuf,
    opened: Mutex<IndexMap<String, Arc<RedbCacheTable>>>,
}

impl RedbCache {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        use std::sync::atomic::{AtomicBool, Ordering};

        let path = path.as_ref().to_path_buf();
        // Opens have been observed taking whole seconds on the caller's
        // thread. Record what this one cost, how big the file was, and
        // whether redb ran a repair pass (a full-file scan triggered by an
        // unclean shutdown) so slow opens are attributable from the log alone.
        let preexisting_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let started = std::time::Instant::now();
        let repaired = Arc::new(AtomicBool::new(false));
        let repaired_flag = repaired.clone();
        let repair_path = path.clone();
        let db = Database::builder()
            .set_repair_callback(move |session| {
                repaired_flag.store(true, Ordering::Relaxed);
                tracing::warn!(
                    target: "vantage_diorama::cache",
                    path = ?repair_path,
                    progress = session.progress(),
                    "redb repair running (unclean shutdown; full-file scan)",
                );
            })
            .create(&path)
            .map_err(|e| {
                error!(
                    "Failed to open redb cache",
                    path = path.display(),
                    detail = e.to_string()
                )
            })?;
        let ms = started.elapsed().as_millis() as u64;
        let repaired = repaired.load(Ordering::Relaxed);
        if ms >= 50 || repaired {
            tracing::warn!(
                target: "vantage_diorama::cache",
                path = ?path,
                ms,
                preexisting_bytes,
                repaired,
                "slow redb cache open",
            );
        } else {
            tracing::debug!(
                target: "vantage_diorama::cache",
                path = ?path,
                ms,
                preexisting_bytes,
                "redb cache open",
            );
        }
        Ok(Self {
            db: Arc::new(db),
            path,
            opened: Mutex::new(IndexMap::new()),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[async_trait]
impl CacheBackend for RedbCache {
    /// Mark the file cleanly closed so the next open skips the repair scan.
    ///
    /// redb records this when the `Database` drops — which, in a GUI process
    /// that exits through the platform's quit path, may never happen: the
    /// handle is reachable from long-lived state whose destructors don't run.
    /// The symptom is a full-file scan on **every** launch, growing with the
    /// cache, and it is invisible except as a slow start.
    ///
    /// An empty write transaction is enough: committing it durably records a
    /// clean state without touching any table, so this is safe to call while
    /// readers still hold tables.
    async fn close(&self) {
        let closed = self
            .db
            .begin_write()
            .map_err(|e| e.to_string())
            .and_then(|txn| txn.commit().map_err(|e| e.to_string()));
        match closed {
            Ok(()) => tracing::debug!(
                target: "vantage_diorama::cache",
                path = ?self.path,
                "cache closed cleanly",
            ),
            // Losing the marker costs a repair scan next time, not data —
            // never worth failing a shutdown over.
            Err(e) => tracing::warn!(
                target: "vantage_diorama::cache",
                path = ?self.path,
                error = %e,
                "could not mark the cache cleanly closed — next open will repair",
            ),
        }
    }

    async fn open_table(&self, name: &str) -> Result<Arc<dyn CacheTable>> {
        let mut opened = self.opened.lock().expect("RedbCache mutex poisoned");
        if let Some(existing) = opened.get(name) {
            return Ok(existing.clone() as Arc<dyn CacheTable>);
        }
        let table = Arc::new(RedbCacheTable::new(self.db.clone(), name.to_string()));
        opened.insert(name.to_string(), table.clone());
        Ok(table as Arc<dyn CacheTable>)
    }

    async fn list_tables(&self) -> Result<Vec<String>> {
        let db = self.db.clone();
        blocking(move || {
            let txn = db
                .begin_read()
                .map_err(|e| error!("redb begin_read failed", detail = e.to_string()))?;
            // Meta tables belong to their cache table, not listed beside it.
            let names = txn
                .list_tables()
                .map_err(|e| error!("redb list_tables failed", detail = e.to_string()))?
                .map(|handle| handle.name().to_string())
                .filter(|name| !name.contains(meta::META_MARKER))
                .collect();
            Ok(names)
        })
        .await
    }

    /// Removes the table's rows and its sibling meta table (stated total,
    /// last refresh) in one transaction, so a table reopened under the same
    /// name starts with no remembered meta.
    async fn drop_table(&self, name: &str) -> Result<()> {
        // Forget the memoized handle first: a later `open_table` under the same
        // name must build a fresh one rather than hand back a handle onto the
        // table we just deleted.
        self.opened
            .lock()
            .expect("RedbCache mutex poisoned")
            .shift_remove(name);
        delete_tables(
            self.db.clone(),
            vec![name.to_string(), meta::meta_table_name(name)],
        )
        .await
    }

    fn name(&self) -> &'static str {
        "redb"
    }
}

/// Delete the redb tables `names` in one transaction. `delete_table` reports
/// whether anything was there; deleting an absent table is a no-op, not a
/// failure.
async fn delete_tables(db: Arc<Database>, names: Vec<String>) -> Result<()> {
    blocking(move || {
        let txn = db
            .begin_write()
            .map_err(|e| error!("redb begin_write failed", detail = e.to_string()))?;
        for name in &names {
            txn.delete_table(table_def(name))
                .map_err(|e| error!("redb delete_table failed", detail = e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| error!("redb commit failed", detail = e.to_string()))?;
        Ok(())
    })
    .await
}
