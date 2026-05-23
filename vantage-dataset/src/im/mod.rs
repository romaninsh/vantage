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
    pub(super) fn get_or_create_table(&self, table_name: &str) -> IndexMap<String, Record<V>> {
        let mut tables = self.tables.lock().unwrap();
        tables.entry(table_name.to_string()).or_default().clone()
    }

    pub(super) fn update_table(&self, table_name: &str, table: IndexMap<String, Record<V>>) {
        let mut tables = self.tables.lock().unwrap();
        tables.insert(table_name.to_string(), table);
    }
}

impl<V> Default for ImDataSource<V> {
    fn default() -> Self {
        Self::new()
    }
}
