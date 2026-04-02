//! # SurrealDB Table Transform Operations
//!
//! This module provides data transformation operations that work with the standard
//! dataset traits, offering functional-style operations on SurrealDB tables.

use serde::{Serialize, de::DeserializeOwned};
use vantage_dataset::dataset::{Result, WritableDataSet};
use vantage_table::{Entity, Table};

use super::{SurrealTableCore, SurrealTableSelectable};
use crate::SurrealDB;

/// Trait for SurrealDB table transform operations
#[async_trait::async_trait]
pub trait SurrealTableTransform<E: Entity>: SurrealTableCore<E> {
    /// Apply a transformation function to all records in the table
    async fn map<F>(self, transform: F) -> Result<Self>
    where
        Self: Sized,
        F: Fn(E) -> E + Send + Sync;

    /// Apply an async transformation function to all records in the table
    async fn map_async<F, Fut>(self, transform: F) -> Result<Self>
    where
        Self: Sized,
        F: Fn(E) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = E> + Send;

    /// Filter records and delete those that don't match the predicate
    async fn filter<F>(self, predicate: F) -> Result<Self>
    where
        Self: Sized,
        F: Fn(&E) -> bool + Send + Sync;

    /// Update records that match a predicate with a transformation
    async fn update_where<P, T>(self, predicate: P, transform: T) -> Result<Self>
    where
        Self: Sized,
        P: Fn(&E) -> bool + Send + Sync,
        T: Fn(E) -> E + Send + Sync;
}

#[async_trait::async_trait]
impl<E> SurrealTableTransform<E> for Table<SurrealDB, E>
where
    E: Entity + Serialize + DeserializeOwned + Send + Sync + 'static,
{
    async fn map<F>(self, transform: F) -> Result<Self>
    where
        Self: Sized,
        F: Fn(E) -> E + Send + Sync,
    {
        // Use the standard WritableDataSet update method
        self.update(move |record| {
            let transformed = transform(record.clone());
            *record = transformed;
        })
        .await?;

        Ok(self)
    }

    async fn map_async<F, Fut>(self, transform: F) -> Result<Self>
    where
        Self: Sized,
        F: Fn(E) -> Fut + Send + Sync,
        Fut: std::future::Future<Output = E> + Send,
    {
        // For async transforms, we need to handle records individually
        let records = self.get_with_ids().await?;

        for (id, record) in records {
            let transformed = transform(record).await;
            self.replace_id(id, transformed).await?;
        }

        Ok(self)
    }

    async fn filter<F>(self, predicate: F) -> Result<Self>
    where
        Self: Sized,
        F: Fn(&E) -> bool + Send + Sync,
    {
        // Get all records with their IDs
        let records = self.get_with_ids().await?;

        // Delete records that DON'T match the predicate
        for (id, record) in records {
            if !predicate(&record) {
                self.delete_id(id).await?;
            }
        }

        Ok(self)
    }

    async fn update_where<P, T>(self, predicate: P, transform: T) -> Result<Self>
    where
        Self: Sized,
        P: Fn(&E) -> bool + Send + Sync,
        T: Fn(E) -> E + Send + Sync,
    {
        // Use the standard WritableDataSet update method with conditional logic
        self.update(move |record| {
            if predicate(record) {
                let transformed = transform(record.clone());
                *record = transformed;
            }
        })
        .await?;

        Ok(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
    struct TestEntity {
        name: String,
        value: i32,
        active: bool,
    }

    impl Entity for TestEntity {}

    #[test]
    fn test_transform_api() {
        // This test demonstrates the intended API usage
        // In a real scenario, you'd have a working SurrealDB connection

        // let db = SurrealDB::new(client);
        // let table = Table::new("test", db).into_entity::<TestEntity>();

        // Test map transformation
        // let updated_table = table.map(|mut entity| {
        //     entity.value += 10;
        //     entity
        // }).await.unwrap();

        // Test async map transformation
        // let updated_table = table.map_async(|entity| async move {
        //     // Simulate async operation
        //     tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        //     TestEntity {
        //         name: format!("async_{}", entity.name),
        //         value: entity.value * 2,
        //         active: entity.active,
        //     }
        // }).await.unwrap();

        // Test filter (keep only active entities)
        // let filtered_table = table.filter(|entity| entity.active).await.unwrap();

        // Test conditional update
        // let updated_table = table.update_where(
        //     |entity| entity.value > 100,  // predicate
        //     |mut entity| {                // transform
        //         entity.name = format!("high_value_{}", entity.name);
        //         entity
        //     }
        // ).await.unwrap();
    }
}
