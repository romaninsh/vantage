use serde_json::{Value, from_value};
use vantage_core::{Result, vantage_error};
use vantage_expressions::protocol::selectable::Selectable;
use vantage_table::{Entity, Table};

use crate::{SurrealDB, associated_query::SurrealAssociated, surreal_return::SurrealReturn};

/// Extension trait for Table<SurrealDB, E> providing SurrealDB-specific async methods
#[async_trait::async_trait]
pub trait SurrealTableExt<E: Entity> {
    /// Create a count query that returns the number of rows
    fn surreal_count(&self) -> SurrealAssociated<SurrealReturn, i64>;

    /// Update record by patching, with specified ID
    async fn update(&self, id: String, patch: Value) -> Result<()>;

    /// Get entities with their IDs as tuples (id, entity)
    async fn get_with_ids(&self) -> Result<Vec<(String, E)>>;

    /// Transform entities for associated table
    async fn map(self, transform: fn(E) -> E) -> Result<Self>
    where
        Self: Sized;

    /// Add a typed column to the table (builder pattern)
    fn with_column_of<T: surreal_client::types::SurrealType>(self, name: impl Into<String>) -> Self
    where
        Self: Sized;

    /// Add a typed column to the table (mutable)
    fn add_column_of<T: surreal_client::types::SurrealType>(&mut self, name: impl Into<String>);
}

#[async_trait::async_trait]
impl<E: Entity> SurrealTableExt<E> for Table<SurrealDB, E> {
    fn surreal_count(&self) -> SurrealAssociated<SurrealReturn, i64> {
        let count_return = self.select().as_count();
        SurrealAssociated::new(count_return, self.data_source().clone())
    }

    async fn get_with_ids(&self) -> Result<Vec<(String, E)>> {
        let s = self.select().with_field("id");

        let data = s.get(self.data_source()).await;
        let mut results = Vec::new();

        for v in data {
            let id = v
                .get("id")
                .ok_or_else(|| vantage_error!("Missing 'id' field in result".to_string()))?
                .as_str()
                .ok_or_else(|| vantage_error!("ID field is not a string"))?
                .to_string();
            let entity = from_value(v.into())
                .map_err(|e| vantage_error!(format!("Failed to deserialize entity: {}", e)))?;
            results.push((id, entity));
        }

        Ok(results)
    }

    async fn update(&self, id: String, patch: Value) -> Result<()> {
        self.data_source()
            .merge(&id, patch)
            .await
            .map_err(|e| vantage_error!("SurrealDB update failed: {}", e))?;
        Ok(())
    }
    async fn map(self, fx: fn(E) -> E) -> Result<Self> {
        for (id, entity) in self.get_with_ids().await? {
            let new_entity = fx(entity.clone());

            // Serialize both entities to Value for comparison
            let original_value = serde_json::to_value(&entity).map_err(|e| {
                vantage_error!(format!("Failed to serialize original entity: {}", e))
            })?;
            let new_value = serde_json::to_value(&new_entity)
                .map_err(|e| vantage_error!("Failed to serialize new entity: {}", e))?;

            // Find differences between original and new entity
            let mut patch = serde_json::Map::new();
            if let (Value::Object(original_map), Value::Object(new_map)) =
                (&original_value, &new_value)
            {
                for (key, new_val) in new_map {
                    if let Some(original_val) = original_map.get(key) {
                        if original_val != new_val {
                            patch.insert(key.clone(), new_val.clone());
                        }
                    } else {
                        // New field added
                        patch.insert(key.clone(), new_val.clone());
                    }
                }
            }

            // Skip if no changes
            if patch.is_empty() {
                continue;
            }

            // Update with the changes
            self.update(id, Value::Object(patch)).await?;
        }
        Ok(self)
    }

    fn with_column_of<T: surreal_client::types::SurrealType>(
        self,
        name: impl Into<String>,
    ) -> Self {
        self.with_column(crate::SurrealColumn::<T>::new(name).into_any())
    }

    fn add_column_of<T: surreal_client::types::SurrealType>(&mut self, name: impl Into<String>) {
        self.add_column(crate::SurrealColumn::<T>::new(name).into_any());
    }
}
