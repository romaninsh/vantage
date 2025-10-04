//! # Vantage Redb Prelude
//!
//! Convenient re-exports for common vantage-redb types and traits.

pub use crate::redb::{Redb, RedbError};
pub use crate::redb_column::{RedbColumn, RedbColumnOperations};
pub use crate::table::RedbTableExt;

// Re-export main redb types for convenience
pub use redb::{
    Database, ReadOnlyTable, ReadTransaction, Table, TableDefinition, WriteTransaction,
};
