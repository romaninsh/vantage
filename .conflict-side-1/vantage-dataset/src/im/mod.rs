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
type TableStorage = Arc<Mutex<HashMap<String, IndexMap<String, Record<serde_json::Value>>>>>;

/// ImDataSource stores tables in memory using IndexMap for ordered iteration
#[derive(Debug, Clone)]
pub struct ImDataSource {
    // table_name -> IndexMap<id, record>
    tables: TableStorage,
}

impl ImDataSource {
    pub fn new() -> Self {
        Self {
            tables: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_or_create_table(&self, table_name: &str) -> IndexMap<String, Record<serde_json::Value>> {
        let mut tables = self.tables.lock().unwrap();
        tables.entry(table_name.to_string()).or_default().clone()
    }

    fn update_table(&self, table_name: &str, table: IndexMap<String, Record<serde_json::Value>>) {
        let mut tables = self.tables.lock().unwrap();
        tables.insert(table_name.to_string(), table);
    }
}

impl Default for ImDataSource {
    fn default() -> Self {
        Self::new()
    }
}
