//! Redb data source — thin wrapper around an `Arc<redb::Database>`, plus
//! the shared read/write helpers consumed by the trait impls in `impls/`.

pub mod impls;

pub(crate) mod helpers;
pub(crate) mod indexes;
pub(crate) mod query;

use redb::{Database, ReadTransaction, TableDefinition, WriteTransaction};
use std::path::Path;
use std::sync::Arc;

use vantage_core::{Result, error};

/// Embedded redb data source. Cloneable (shares the inner `Arc<Database>`).
#[derive(Clone, Debug)]
pub struct Redb {
    db: Arc<Database>,
}

impl Redb {
    /// Open or create a redb database at the given path.
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = Database::create(path)
            .map_err(|e| error!("Failed to create redb database", details = e.to_string()))?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Open an existing redb database. Errors if the file doesn't exist.
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db = Database::open(path)
            .map_err(|e| error!("Failed to open redb database", details = e.to_string()))?;
        Ok(Self { db: Arc::new(db) })
    }

    /// Wrap an existing `redb::Database` (e.g. for in-memory tests via builder).
    pub fn from_database(db: Database) -> Self {
        Self { db: Arc::new(db) }
    }

    /// Borrow the underlying database.
    pub fn database(&self) -> &Database {
        &self.db
    }

    pub(crate) fn begin_read(&self) -> Result<ReadTransaction> {
        self.db
            .begin_read()
            .map_err(|e| error!("Failed to begin redb read txn", details = e.to_string()))
    }

    pub(crate) fn begin_write(&self) -> Result<WriteTransaction> {
        self.db
            .begin_write()
            .map_err(|e| error!("Failed to begin redb write txn", details = e.to_string()))
    }
}

// Table-definition helpers — keep the name → TableDefinition mapping in one place.
//
// The K/V type parameters need an explicit `'static` lifetime because redb's
// `open_table` requires `K: Key + 'static` and `V: Value + 'static`. Without
// the annotation Rust infers them as the lifetime of the `name` argument,
// which is what produced the "borrowed data escapes" errors.

/// Main row store: `id (utf-8 string) → cbor row body`.
pub(crate) fn main_table_def(name: &str) -> TableDefinition<'_, &'static str, &'static [u8]> {
    TableDefinition::new(name)
}

/// Secondary index: `(cbor value bytes, id) → ()`. Composite key gives us
/// non-unique indexes for free via redb's range scan.
pub(crate) fn index_table_name(table_name: &str, column_name: &str) -> String {
    format!("{}__idx__{}", table_name, column_name)
}

pub(crate) fn index_table_def(
    name: &str,
) -> TableDefinition<'_, (&'static [u8], &'static str), ()> {
    TableDefinition::new(name)
}
