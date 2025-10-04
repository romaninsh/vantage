//! # Vantage redb Extension
//!
//! Extends Vantage by adding support for redb key-value database.
//! Unlike SQL databases, redb is a key-value store with ACID transactions
//! and support for multiple tables (key spaces).
//!
//! ## Features
//!
//! - ACID transactions
//! - Multiple tables for organizing data
//! - Secondary indexes through separate tables
//! - Serialization/deserialization with serde
//!
//! ## Quick Start
//!
//! ```rust
//! use vantage_redb::RedbDB;
//! use redb::TableDefinition;
//!
//! const USERS: TableDefinition<&str, &[u8]> = TableDefinition::new("users");
//!
//! let db = RedbDB::new("my_db.redb").unwrap();
//! // Use db for CRUD operations
//! ```

pub mod prelude;
pub mod redb;
pub mod redb_column;
pub mod table;

pub use redb::{Redb, RedbError};
pub use redb_column::{RedbColumn, RedbColumnOperations};
pub use table::{RedbTableCore, RedbTableExt};
