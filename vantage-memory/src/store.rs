//! The store: named tables with a synchronous API. Sims and other threads
//! call it directly; the async trait layers wrap it.

mod events;
pub(crate) mod ids;
mod index;
mod query;
mod table;
mod write;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use parking_lot::RwLock;
use tokio::sync::Mutex;
use vantage_types::Record;

pub use events::MemoryChange;
pub use table::MemoryTable;
pub use write::UpsertOutcome;

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

/// How long a transaction waits for another one to finish with the store.
const TRANSACTION_WAIT: Duration = Duration::from_secs(2);

#[derive(Clone)]
pub struct MemoryStore {
    tables: Arc<RwLock<HashMap<String, MemoryTableHandle>>>,
    /// Held by the transaction working on the store, from its first use
    /// until it finishes; callers outside a transaction never take it.
    claim: Arc<Mutex<()>>,
    transaction_wait: Duration,
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self {
            tables: Arc::default(),
            claim: Arc::default(),
            transaction_wait: TRANSACTION_WAIT,
        }
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// How long a transaction's first use of the store waits for another
    /// transaction to finish with it before failing (2 s by default). Two
    /// blocks that take two stores in opposite orders would otherwise wait
    /// on each other for good.
    pub fn with_transaction_wait(mut self, wait: Duration) -> Self {
        self.transaction_wait = wait;
        self
    }

    pub(crate) fn claim(&self) -> &Arc<Mutex<()>> {
        &self.claim
    }

    pub(crate) fn transaction_wait(&self) -> Duration {
        self.transaction_wait
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

    /// The table `name` when it exists; never creates one.
    pub(crate) fn existing_table(&self, name: &str) -> Option<MemoryTableHandle> {
        self.tables.read().get(name).cloned()
    }

    pub fn table_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.tables.read().keys().cloned().collect();
        names.sort();
        names
    }
}
