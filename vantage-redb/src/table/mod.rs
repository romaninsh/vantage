//! # Redb Table Extensions
//!
//! This module provides redb-specific extensions for `Table<RedbDB, E>`.
//! Since redb is a key-value store, the extensions focus on:
//!
//! - **Key-value operations** - Direct get/set operations
//! - **Index management** - Secondary indexes through separate tables
//! - **Transaction support** - ACID transactions for data consistency
//! - **Serialization** - Automatic serde support for entities


