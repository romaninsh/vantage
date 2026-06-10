// src/im/mod.rs

use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use vantage_types::Record;

pub mod dataset_insertable;
pub mod dataset_readable;
pub mod dataset_writable;
pub mod im_table;

pub mod valueset_insertable;
pub mod valueset_readable;
pub mod valueset_writable;
pub use im_table::ImTable;

/// Type alias for the complex table storage structure
type TableStorage<V> = Arc<Mutex<HashMap<String, IndexMap<String, Record<V>>>>>;

/// In-memory data source storing tables as nested maps, keyed first by table
/// name then by row id. Generic over the wire value type `V` so the same
/// storage primitive can hold `serde_json::Value` records (the original
/// entity-friendly mode) or `ciborium::Value` records (used by
/// `MockTableSource` to participate in the CBOR-typed `TableSource` /
/// `Vista` machinery). Defaults to `serde_json::Value` for back-compat.
#[derive(Debug)]
pub struct ImDataSource<V = serde_json::Value> {
    // table_name -> IndexMap<id, record>
    tables: TableStorage<V>,
}

impl<V> Clone for ImDataSource<V> {
    fn clone(&self) -> Self {
        Self {
            tables: self.tables.clone(),
        }
    }
}

impl<V> ImDataSource<V> {
    pub fn new() -> Self {
        Self {
            tables: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<V: Clone> ImDataSource<V> {
    /// Run `f` against an immutable view of the named table, holding the lock
    /// for its duration. Missing tables present as empty (without being
    /// created). Clone only what you need to return — the borrow ends with `f`.
    pub(super) fn with_table<R>(
        &self,
        table_name: &str,
        f: impl FnOnce(&IndexMap<String, Record<V>>) -> R,
    ) -> R {
        let tables = self.tables.lock().unwrap();
        match tables.get(table_name) {
            Some(table) => f(table),
            None => f(&IndexMap::new()),
        }
    }

    /// Number of rows currently stored in the named table (`0` if it has never
    /// been written to). Synchronous and clone-free — reads the row count under
    /// the storage lock. Lets sync callers (e.g. `MockTableSource`) derive a
    /// count from the single source of truth instead of a side store.
    pub fn table_len(&self, table_name: &str) -> usize {
        self.with_table(table_name, |table| table.len())
    }

    /// Run `f` against a mutable view of the named table (created on demand),
    /// holding the lock across the whole read-modify-write so concurrent
    /// writers can't clobber each other's changes.
    pub(super) fn with_table_mut<R>(
        &self,
        table_name: &str,
        f: impl FnOnce(&mut IndexMap<String, Record<V>>) -> R,
    ) -> R {
        let mut tables = self.tables.lock().unwrap();
        let table = tables.entry(table_name.to_string()).or_default();
        f(table)
    }
}

impl<V> Default for ImDataSource<V> {
    fn default() -> Self {
        Self::new()
    }
}
