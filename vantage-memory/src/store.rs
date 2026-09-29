//! The store: named tables with a synchronous API. Sims and other threads
//! call it directly; the async trait layers wrap it.

mod events;
mod ids;
mod index;
mod query;
mod table;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::sync::Arc;

use ciborium::Value as CborValue;
use parking_lot::RwLock;
use vantage_types::Record;

pub use events::MemoryChange;
pub use table::MemoryTable;

/// A stored row, shared between the table, readers and change events.
pub type Row = Arc<Record<CborValue>>;
pub type MemoryTableHandle = Arc<MemoryTable>;

/// How a table stores and identifies rows.
#[derive(Clone, Debug)]
pub struct TableDef {
    pub id_column: String,
    /// Columns with a hash index, used by `Eq` / `InSet` lookups.
    pub indexed: Vec<String>,
    /// Prefix for generated ids (`"T-"` gives `T-1`, `T-2`, …).
    pub id_prefix: Option<String>,
}

impl Default for TableDef {
    fn default() -> Self {
        Self {
            id_column: "id".into(),
            indexed: Vec::new(),
            id_prefix: None,
        }
    }
}

#[derive(Clone, Default)]
pub struct MemoryStore {
    tables: Arc<RwLock<HashMap<String, MemoryTableHandle>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// The table `name`, created with the default definition if missing.
    pub fn table(&self, name: &str) -> MemoryTableHandle {
        self.define(name, TableDef::default())
    }

    /// The table `name`, created with `def` if missing. An existing table
    /// keeps its original definition.
    pub fn define(&self, name: &str, def: TableDef) -> MemoryTableHandle {
        if let Some(t) = self.tables.read().get(name) {
            return t.clone();
        }
        self.tables
            .write()
            .entry(name.to_string())
            .or_insert_with(|| Arc::new(MemoryTable::new(name.to_string(), def)))
            .clone()
    }

    pub fn table_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tables.read().keys().cloned().collect();
        names.sort();
        names
    }
}
