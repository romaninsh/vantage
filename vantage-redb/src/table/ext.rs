//! # Redb Table Extensions
//!
//! This module provides redb-specific extensions for `Table<RedbDB, E>`.
//! Extensions focus on key-value operations with secondary indexes.

use async_trait::async_trait;
use serde_json::Value;
use vantage_expressions::util::error::Result;
use vantage_table::{Entity, Table};

use crate::Redb;

/// Extension trait for Table<RedbDB, E> providing redb-specific async methods
#[async_trait]
pub trait RedbTableExt<E: Entity> {
    /// Get entities with their IDs as tuples (id, entity)
    async fn get_with_ids(&self) -> Result<Vec<(String, E)>>;

    /// Update record by patching, with specified ID
    async fn update(&self, id: String, patch: Value) -> Result<()>;

    /// Get count of records in table
    async fn count(&self) -> Result<i64>;

    /// Get records by column value using secondary index
    async fn get_by_column(&self, column: &str, value: Value) -> Result<Vec<E>>;
}

#[async_trait]
impl<E: Entity> RedbTableExt<E> for Table<Redb, E> {
    async fn get_with_ids(&self) -> Result<Vec<(String, E)>> {
        todo!("Implement get_with_ids for redb table")
    }

    async fn update(&self, _id: String, _patch: Value) -> Result<()> {
        todo!("Implement update for redb table")
    }

    async fn count(&self) -> Result<i64> {
        todo!("Implement count for redb table")
    }

    async fn get_by_column(&self, _column: &str, _value: Value) -> Result<Vec<E>> {
        todo!("Implement get_by_column for redb table")
    }
}
