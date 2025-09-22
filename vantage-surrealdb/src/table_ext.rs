use serde_json::Value;
use vantage_expressions::{
    expr,
    protocol::{
        datasource::{ColumnLike, DataSource},
        selectable::Selectable,
    },
    util::error::{Error, Result},
};
use vantage_table::{Entity, Table};

use crate::{
    SurrealDB, associated_query::SurrealAssociated, select::SurrealSelect,
    surreal_return::SurrealReturn,
};
use vantage_expressions::protocol::result;

/// Extension trait for Table<SurrealDB, E> providing SurrealDB-specific query methods
#[async_trait::async_trait]
pub trait SurrealTableExt<E: Entity> {
    /// Create a SurrealAssociated that returns multiple rows with all columns
    fn select_surreal(&self) -> SurrealAssociated<SurrealSelect<result::Rows>, Vec<E>>;

    /// Create a SurrealAssociated that returns the first row with all columns
    fn select_surreal_first(&self) -> SurrealAssociated<SurrealSelect<result::SingleRow>, E>;

    /// Create a SurrealAssociated that returns a single column from all rows
    fn select_surreal_column(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealAssociated<SurrealSelect<result::List>, Vec<serde_json::Value>>>;

    /// Create a SurrealAssociated that returns a single value (first row, single column)
    fn select_surreal_single(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealAssociated<SurrealSelect<result::Single>, serde_json::Value>>;

    /// Execute a select query and return the results directly
    async fn surreal_get(&self) -> Result<Vec<E>>;

    /// Create a count query that returns the number of rows
    fn surreal_count(&self) -> SurrealAssociated<SurrealReturn, i64>;

    /// Update record by patching, with specified ID
    async fn update(&self, id: String, patch: Value) -> Result<()>;

    /// Get entities with their IDs as tuples (id, entity)
    async fn get_with_ids(&self) -> Result<Vec<(String, E)>>;

    async fn map(self, transform: fn(E) -> E) -> Result<Self>
    where
        Self: Sized;
}

#[async_trait::async_trait]
impl<E: Entity> SurrealTableExt<E> for Table<SurrealDB, E> {
    fn select_surreal(&self) -> SurrealAssociated<SurrealSelect<result::Rows>, Vec<E>> {
        let mut select = SurrealSelect::new();

        select.set_source(self.table_name(), None);

        for column in self.columns().values() {
            match column.alias() {
                Some(alias) => select.add_expression(expr!(column.name()), Some(alias.to_string())),
                None => select.add_field(column.name()),
            }
        }

        for condition in self.conditions() {
            select.add_where_condition(condition.clone());
        }

        SurrealAssociated::new(select, self.data_source().clone())
    }

    fn select_surreal_first(&self) -> SurrealAssociated<SurrealSelect<result::SingleRow>, E> {
        let mut select = SurrealSelect::new();

        select.set_source(self.table_name(), None);

        for column in self.columns().values() {
            match column.alias() {
                Some(alias) => select.add_expression(expr!(column.name()), Some(alias.to_string())),
                None => select.add_field(column.name()),
            }
        }

        for condition in self.conditions() {
            select.add_where_condition(condition.clone());
        }

        let single_row_select = select.only_first_row();
        SurrealAssociated::new(single_row_select, self.data_source().clone())
    }

    fn select_surreal_column(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealAssociated<SurrealSelect<result::List>, Vec<serde_json::Value>>> {
        let column_name = column.into();

        // Validate column exists
        if !self.columns().contains_key(&column_name) {
            return Err(Error::new(format!(
                "Column '{}' not found in table",
                column_name
            )));
        }

        let column_obj = &self.columns()[&column_name];
        let mut select = SurrealSelect::new();

        select.set_source(self.table_name(), None);

        // Add only the requested column
        let mut list_select = select.only_column(column_obj.name());

        for condition in self.conditions() {
            list_select.add_where_condition(condition.clone());
        }

        Ok(SurrealAssociated::new(
            list_select,
            self.data_source().clone(),
        ))
    }

    fn select_surreal_single(
        &self,
        column: impl Into<String>,
    ) -> Result<SurrealAssociated<SurrealSelect<result::Single>, serde_json::Value>> {
        let column_name = column.into();

        // Validate column exists
        if !self.columns().contains_key(&column_name) {
            return Err(Error::new(format!(
                "Column '{}' not found in table",
                column_name
            )));
        }

        let column_obj = &self.columns()[&column_name];
        let mut select = SurrealSelect::new();

        select.set_source(self.table_name(), None);

        for condition in self.conditions() {
            select.add_where_condition(condition.clone());
        }

        let single_select = select.only_first_row().only_column(column_obj.name());
        Ok(SurrealAssociated::new(
            single_select,
            self.data_source().clone(),
        ))
    }

    async fn surreal_get(&self) -> Result<Vec<E>> {
        use vantage_expressions::AssociatedQueryable;
        self.select_surreal().get().await
    }

    fn surreal_count(&self) -> SurrealAssociated<SurrealReturn, i64> {
        let count_return = self.select_surreal().query.as_count();
        SurrealAssociated::new(count_return, self.data_source().clone())
    }

    async fn update(&self, id: String, patch: Value) -> Result<()> {
        self.data_source()
            .merge(&id, patch)
            .await
            .map_err(|e| Error::new(format!("SurrealDB update failed: {}", e)))?;
        Ok(())
    }

    async fn get_with_ids(&self) -> Result<Vec<(String, E)>> {
        use crate::identifier::Identifier;
        use crate::select::select_field::SelectField;

        // Use the existing select_surreal() logic and add id field at the beginning
        let mut select = self.select_surreal();

        // Insert id field at the beginning to match expected query pattern
        select
            .query
            .fields
            .insert(0, SelectField::new(Identifier::new("id")));

        // Execute the query and get raw values
        let raw_result = self.data_source().execute(&select.query.into()).await;

        // Parse results
        let values = if let serde_json::Value::Array(items) = raw_result {
            items
        } else {
            return Err(Error::new("Expected array of objects from database"));
        };

        let mut results = Vec::new();
        for item in values {
            // Extract ID from the JSON object
            let id = if let serde_json::Value::Object(ref obj) = item {
                obj.get("id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| Error::new("Entity missing 'id' field".to_string()))?
                    .to_string()
            } else {
                return Err(Error::new("Expected object from database".to_string()));
            };

            // Create a copy without the id field for entity deserialization
            let mut entity_data = item.clone();
            if let serde_json::Value::Object(ref mut obj) = entity_data {
                obj.remove("id");
            }

            // Deserialize the entity
            let entity = serde_json::from_value::<E>(entity_data)
                .map_err(|e| Error::new(format!("Failed to deserialize entity: {}", e)))?;

            results.push((id, entity));
        }

        Ok(results)
    }

    async fn map(self, fx: fn(E) -> E) -> Result<Self> {
        for (id, entity) in self.get_with_ids().await? {
            let new_entity = fx(entity.clone());

            // Serialize both entities to Value for comparison
            let original_value = serde_json::to_value(&entity)
                .map_err(|e| Error::new(format!("Failed to serialize original entity: {}", e)))?;
            let new_value = serde_json::to_value(&new_entity)
                .map_err(|e| Error::new(format!("Failed to serialize new entity: {}", e)))?;

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
}
