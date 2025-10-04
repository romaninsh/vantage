use async_trait::async_trait;
use serde::de::DeserializeOwned;
use vantage_dataset::dataset::{DataSetError, Id, ReadableDataSet, Result};
use vantage_expressions::expr;
use vantage_table::{Entity, Table};

use crate::{SurrealColumn, SurrealColumnOperations, SurrealDB, SurrealTableExt, thing::Thing};

#[async_trait]
impl<E> ReadableDataSet<E> for Table<SurrealDB, E>
where
    E: Entity + DeserializeOwned + Send + Sync + 'static,
{
    async fn get(&self) -> Result<Vec<E>> {
        // Use the existing SurrealTableExt functionality
        let query_result = self.select_surreal();

        match query_result.get().await {
            Ok(entities) => Ok(entities),
            Err(_) => Err(DataSetError::other("Failed to execute SurrealDB query")),
        }
    }

    async fn get_some(&self) -> Result<Option<E>> {
        // Get first record using SurrealDB query
        let query_result = self.select_surreal_first();

        match query_result.get().await {
            Ok(entity) => Ok(Some(entity)),
            Err(_) => Ok(None),
        }
    }

    async fn get_id(&self, id: impl Id) -> Result<E> {
        self.clone()
            .with_id(id)
            .get_some()
            .await?
            .ok_or_else(|| DataSetError::other("Record not found"))
    }

    async fn get_as<T>(&self) -> Result<Vec<T>>
    where
        T: DeserializeOwned,
    {
        // Use SurrealTableExt to execute query and get raw results
        let mut select = self.data_source().select();
        select.set_source(self.table_name(), None);

        // Add all configured columns
        for column in self.columns().values() {
            match column.alias() {
                Some(alias) => select.add_expression(column.expr(), Some(alias.to_string())),
                None => select.add_field(column.name()),
            }
        }

        // Add all conditions from the table
        for condition in self.conditions() {
            select.add_where_condition(condition.clone());
        }

        let raw_result = self.data_source().execute(&select.into()).await;

        // Parse as array of T
        if let serde_json::Value::Array(items) = raw_result {
            let entities = items
                .into_iter()
                .map(|item| serde_json::from_value::<T>(item))
                .collect::<std::result::Result<Vec<T>, _>>()
                .map_err(|e| {
                    DataSetError::other(format!("Failed to deserialize entities: {}", e))
                })?;
            return Ok(entities);
        }

        Err(DataSetError::other(
            "Expected array of objects from SurrealDB",
        ))
    }

    async fn get_some_as<T>(&self) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        // Get first record as type T
        let mut select = self.data_source().select();
        select.set_source(self.table_name(), None);

        // Add all configured columns
        for column in self.columns().values() {
            match column.alias() {
                Some(alias) => select.add_expression(column.expr(), Some(alias.to_string())),
                None => select.add_field(column.name()),
            }
        }

        // Add all conditions from the table
        for condition in self.conditions() {
            select.add_where_condition(condition.clone());
        }

        // Limit to 1 record for efficiency
        select.add_limit(1);

        let raw_result = self.data_source().execute(&select.into()).await;

        // Parse the first result
        if let serde_json::Value::Array(items) = raw_result {
            if let Some(first_item) = items.first() {
                let entity = serde_json::from_value::<T>(first_item.clone()).map_err(|e| {
                    DataSetError::other(format!("Failed to deserialize entity: {}", e))
                })?;
                return Ok(Some(entity));
            }
        }

        Ok(None)
    }

    async fn get_values(&self) -> Result<Vec<serde_json::Value>> {
        // Get raw data as JSON values for indexing operations
        let mut select = self.data_source().select();
        select.set_source(self.table_name(), None);

        // Add all configured columns
        for column in self.columns().values() {
            match column.alias() {
                Some(alias) => select.add_expression(column.expr(), Some(alias.to_string())),
                None => select.add_field(column.name()),
            }
        }

        // Add all conditions from the table
        for condition in self.conditions() {
            select.add_where_condition(condition.clone());
        }

        let raw_result = self.data_source().execute(&select.into()).await;

        // Return as array of JSON values
        if let serde_json::Value::Array(items) = raw_result {
            Ok(items)
        } else {
            Err(DataSetError::other(
                "Expected array of objects from SurrealDB",
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use vantage_table::TableSource;

    #[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
    struct User {
        id: String,
        name: String,
        email: String,
    }

    impl Entity for User {}

    // Note: These tests would need a mock SurrealDB instance to run properly
    // For now, they demonstrate the intended API usage

    #[tokio::test]
    #[ignore] // Requires actual SurrealDB connection
    async fn test_get_id_optimized() {
        // This test would require setting up a real SurrealDB connection
        // let surrealdb = SurrealDB::new(client);
        // let users = Table::new("users", surrealdb)
        //     .with_column("id")
        //     .with_column("name")
        //     .with_column("email")
        //     .into_entity::<User>();

        // Should be able to get user by ID efficiently
        // let user = users.get_id("user-123").await.unwrap();
        // assert_eq!(user.id, "user-123");
    }
}
