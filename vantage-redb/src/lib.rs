//! # vantage-redb
//!
//! Embedded redb key-value persistence for the Vantage framework.
//!
//! Implements [`vantage_table::TableSource`] over [redb], with full CRUD,
//! ACID write transactions, and column-driven secondary indexes maintained
//! atomically alongside main rows.
//!
//! ## Capabilities
//!
//! - CBOR row bodies that preserve `RedbTypeVariants` tags through
//!   round-trip.
//! - Secondary indexes opt-in via `ColumnFlag::Indexed`. Index tables use
//!   redb's composite keys `(value_bytes, id)` for non-unique columns.
//! - Conditions limited to `eq` / `in_` on indexed columns (or the table's
//!   id column, which short-circuits to a direct main-table lookup).
//! - No query builder — redb has no query language.

pub mod condition;
pub mod operation;
pub mod prelude;
pub mod redb;
pub mod types;

pub use condition::RedbCondition;
pub use operation::RedbOperation;
pub use redb::Redb;
pub use types::{AnyRedbType, RedbType, RedbTypeVariants};
