// src/im/mod.rs

use indexmap::IndexMap;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub mod dataset_insertable;
pub mod dataset_readable;
pub mod dataset_writable;
pub mod table;
pub mod valueset_readable;
pub mod valueset_writable;
pub use table::ImTable;

/// ImDataSource stores tables in memory using IndexMap for ordered iteration
#[derive(Debug, Clone)]
pub struct ImDataSource {
    // table_name -> IndexMap<id, serialized_record>
    tables: Arc<Mutex<HashMap<String, IndexMap<String, serde_json::Value>>>>,
}

impl ImDataSource {
    pub fn new() -> Self {
        Self {
            tables: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_or_create_table(&self, table_name: &str) -> IndexMap<String, serde_json::Value> {
        let mut tables = self.tables.lock().unwrap();
        tables.entry(table_name.to_string()).or_default().clone()
    }

    fn update_table(&self, table_name: &str, table: IndexMap<String, serde_json::Value>) {
        let mut tables = self.tables.lock().unwrap();
        tables.insert(table_name.to_string(), table);
    }
}

impl Default for ImDataSource {
    fn default() -> Self {
        Self::new()
    }
}
