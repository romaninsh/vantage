use crate::{SurrealDB, thing::Thing};
use async_trait::async_trait;
use vantage_core::{error, util::error::Context};
use vantage_expressions::Expression;
use vantage_table::Table;

#[async_trait]
impl vantage_table::TableSource for SurrealDB {
    type Column = crate::SurrealColumn;
    type Expr = Expression;

    fn create_column(&self, name: &str, _table: impl vantage_table::TableLike) -> Self::Column {
        crate::SurrealColumn::new(name)
    }

    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<vantage_expressions::protocol::expressive::IntoExpressive<Self::Expr>>,
    ) -> Self::Expr {
        Expression::new(template, parameters)
    }

    fn search_expression(
        &self,
        table: &impl vantage_table::TableLike,
        search_value: &str,
    ) -> Self::Expr {
        // SurrealDB uses CONTAINS operator for string search
        let columns = table.columns();

        // Search in "name" field if it exists, otherwise use first string column
        if columns.contains_key("name") {
            Expression::new("name CONTAINS {}", vec![search_value.into()])
        } else {
            // Default to searching first column
            if let Some((col_name, _)) = columns.first() {
                Expression::new(
                    &format!("{} CONTAINS {{}}", col_name),
                    vec![search_value.into()],
                )
            } else {
                // No columns, return always-true expression
                Expression::new("true", vec![])
            }
        }
    }

    async fn get_table_data<E>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Vec<(String, E)>>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        let select = table.select();
        let raw_result = select.get(self).await;

        let mut results = Vec::new();
        for item in raw_result {
            let id = item
                .get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown")
                .to_string();

            let value = serde_json::Value::Object(item.clone());
            let table_name = table.table_name().to_string();
            let entity_type = std::any::type_name::<E>();
            let entity = serde_path_to_error::deserialize::<_, E>(value).map_err(|e| {
                error!(
                    format!("Failed to deserialize entity: {}", e.inner()),
                    table = &table_name,
                    entity_type = entity_type,
                    field = e.path().to_string()
                )
            })?;

            results.push((id, entity));
        }

        Ok(results)
    }

    async fn get_table_data_some<E>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Option<(String, E)>>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        let select = table.select().only_first_row();
        let raw_result = select.try_get(self).await;

        match raw_result {
            Some(map) => {
                let id = map
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_string();

                let value = serde_json::Value::Object(map.clone());
                let table_name = table.table_name().to_string();
                let entity_type = std::any::type_name::<E>();
                let entity = serde_path_to_error::deserialize::<_, E>(value).map_err(|e| {
                    error!(
                        format!("Failed to deserialize entity: {}", e.inner()),
                        table = &table_name,
                        entity_type = entity_type,
                        field = e.path().to_string()
                    )
                })?;

                Ok(Some((id, entity)))
            }
            None => Ok(None),
        }
    }

    async fn get_table_data_as_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Vec<serde_json::Value>>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        let select = table.select();
        let raw_result = select.get(self).await;

        let values = raw_result
            .into_iter()
            .map(serde_json::Value::Object)
            .collect();

        Ok(values)
    }

    async fn insert_table_data<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
        record: E,
    ) -> vantage_dataset::dataset::Result<Option<String>>
    where
        E: vantage_core::Entity + serde::Serialize,
        Self: Sized,
    {
        let data = serde_json::to_value(record).context("Failed to serialize record")?;

        let table_obj = surreal_client::Table::new(table.table_name());
        let client = self.inner.lock().await;
        let result = client
            .insert(&table_obj.to_string(), data)
            .await
            .context("Failed to insert record")?;

        // Extract ID from result - SurrealDB typically returns the inserted record with ID
        if let Some(obj) = result.as_object()
            && let Some(id_val) = obj.get("id")
            && let Some(id_str) = id_val.as_str()
        {
            return Ok(Some(id_str.to_string()));
        }

        Ok(None)
    }

    async fn insert_table_data_with_id<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
        id: impl vantage_dataset::dataset::Id,
        record: E,
    ) -> vantage_dataset::dataset::Result<()>
    where
        E: vantage_core::Entity + serde::Serialize,
        Self: Sized,
    {
        let data = serde_json::to_value(record).context("Failed to serialize record")?;
        let thing = Thing::new(table.table_name(), id.into());
        let record_id: surreal_client::RecordId = thing.into();

        let client = self.inner.lock().await;
        client
            .create(&record_id.to_string(), Some(data))
            .await
            .context("Failed to insert record with ID")?;

        Ok(())
    }

    async fn replace_table_data_with_id<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
        id: impl vantage_dataset::dataset::Id,
        record: E,
    ) -> vantage_dataset::dataset::Result<()>
    where
        E: vantage_core::Entity + serde::Serialize,
        Self: Sized,
    {
        let data = serde_json::to_value(record).context("Failed to serialize record")?;
        let id_string = id.into();

        // Handle both full record IDs and bare IDs
        let thing = if id_string.contains(':') {
            id_string.parse::<Thing>().map_err(|e| {
                vantage_core::util::error::vantage_error!("Invalid Thing format: {}", e)
            })?
        } else {
            Thing::new(table.table_name(), id_string)
        };

        let record_id: surreal_client::RecordId = thing.into();

        let client = self.inner.lock().await;
        client
            .update_record(record_id, data)
            .await
            .context("Failed to update record")?;

        Ok(())
    }

    async fn patch_table_data_with_id<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
        id: impl vantage_dataset::dataset::Id,
        partial: serde_json::Value,
    ) -> vantage_dataset::dataset::Result<()>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        let thing = Thing::new(table.table_name(), id.into());
        let record_id: surreal_client::RecordId = thing.into();

        // Convert JSON value to JSON Patch format
        let patches = if let serde_json::Value::Object(map) = partial {
            map.into_iter()
                .map(|(key, value)| {
                    serde_json::json!({
                        "op": "replace",
                        "path": format!("/{}", key),
                        "value": value
                    })
                })
                .collect::<Vec<_>>()
        } else {
            return Err(vantage_core::util::error::vantage_error!(
                "Patch data must be an object"
            ));
        };

        let client = self.inner.lock().await;
        client
            .patch(&record_id.to_string(), patches)
            .await
            .context("Failed to patch record")?;

        Ok(())
    }

    async fn delete_table_data_with_id<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
        id: impl vantage_dataset::dataset::Id,
    ) -> vantage_dataset::dataset::Result<()>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        let thing = Thing::new(table.table_name(), id.into());
        let record_id: surreal_client::RecordId = thing.into();

        let client = self.inner.lock().await;
        client
            .delete_record(record_id)
            .await
            .context("Failed to delete record")?;

        Ok(())
    }

    async fn update_table_data<E, F>(
        &self,
        _table: &vantage_table::Table<Self, E>,
        _callback: F,
    ) -> vantage_dataset::dataset::Result<()>
    where
        E: vantage_core::Entity,
        F: Fn(&mut E) + Send + Sync,
        Self: Sized,
    {
        // TODO: Implement bulk update with callback
        todo!("update_table_data not yet implemented")
    }

    async fn delete_table_data<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<()>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        let table_obj = surreal_client::Table::new(table.table_name());
        let client = self.inner.lock().await;
        client
            .delete_all(table_obj)
            .await
            .context("Failed to delete all records from table")?;

        Ok(())
    }

    async fn get_table_data_by_id<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
        id: impl vantage_dataset::dataset::Id,
    ) -> vantage_dataset::dataset::Result<E>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        let id_string = id.into();

        // Handle both full record IDs (e.g., "client:biff") and bare IDs (e.g., "biff")
        let thing = if id_string.contains(':') {
            // Already a full record ID, parse it directly
            id_string.parse::<Thing>().map_err(|e| {
                vantage_core::util::error::vantage_error!("Invalid Thing format: {}", e)
            })?
        } else {
            // Bare ID, construct with table name
            Thing::new(table.table_name(), id_string)
        };

        let record_id: surreal_client::RecordId = thing.into();

        let client = self.inner.lock().await;
        let raw_result = client
            .select_record(record_id)
            .await
            .with_context(|| error!("Failed to get record by ID"))?;

        let table_name = table.table_name().to_string();
        let entity_type = std::any::type_name::<E>();
        let entity = serde_path_to_error::deserialize::<_, E>(raw_result.clone()).map_err(|e| {
            error!(
                format!("Failed to deserialize entity: {}", e.inner()),
                table = &table_name,
                entity_type = entity_type,
                field = e.path().to_string()
            )
        })?;

        Ok(entity)
    }

    async fn get_table_data_as_value_by_id<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
        id: &str,
    ) -> vantage_dataset::dataset::Result<serde_json::Value>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        // Handle both full record IDs (e.g., "client:biff") and bare IDs (e.g., "biff")
        let thing = if id.contains(':') {
            // Already a full record ID, parse it directly
            id.parse::<Thing>().map_err(|e| {
                vantage_core::util::error::vantage_error!("Invalid Thing format: {}", e)
            })?
        } else {
            // Bare ID, construct with table name
            Thing::new(table.table_name(), id.to_string())
        };

        let record_id: surreal_client::RecordId = thing.into();

        let client = self.inner.lock().await;
        let raw_result = client
            .select_record(record_id)
            .await
            .context("Failed to get record by ID")?;

        Ok(raw_result)
    }

    async fn get_table_data_as_value_some<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<Option<serde_json::Value>>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        let select = table.select().only_first_row();
        let raw_result = select.try_get(self).await;

        match raw_result {
            Some(map) => Ok(Some(serde_json::Value::Object(map))),
            None => Ok(None),
        }
    }

    async fn insert_table_data_with_id_value<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
        id: &str,
        record: serde_json::Value,
    ) -> vantage_dataset::dataset::Result<()>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        let thing = Thing::new(table.table_name(), id.to_string());
        let record_id: surreal_client::RecordId = thing.into();

        let client = self.inner.lock().await;
        client
            .create(&record_id.to_string(), Some(record))
            .await
            .context("Failed to insert record with ID using value")?;

        Ok(())
    }

    async fn replace_table_data_with_id_value<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
        id: &str,
        record: serde_json::Value,
    ) -> vantage_dataset::dataset::Result<()>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        // Handle both full record IDs and bare IDs
        let thing = if id.contains(':') {
            id.parse::<Thing>().map_err(|e| {
                vantage_core::util::error::vantage_error!("Invalid Thing format: {}", e)
            })?
        } else {
            Thing::new(table.table_name(), id.to_string())
        };

        let record_id: surreal_client::RecordId = thing.into();

        let client = self.inner.lock().await;
        client
            .update_record(record_id, record)
            .await
            .context("Failed to replace record using value")?;

        Ok(())
    }

    async fn update_table_data_value<E, F>(
        &self,
        table: &vantage_table::Table<Self, E>,
        callback: F,
    ) -> vantage_dataset::dataset::Result<()>
    where
        E: vantage_core::Entity,
        F: Fn(&mut serde_json::Value) + Send + Sync,
        Self: Sized,
    {
        // Get all records as values
        let values = self.get_table_data_as_value(table).await?;

        // Apply callback to each value and update
        for mut value in values {
            callback(&mut value);

            // Extract ID from the value to update the record
            if let Some(id_value) = value.get("id")
                && let Some(id_str) = id_value.as_str()
            {
                let thing = if id_str.contains(':') {
                    id_str.parse::<Thing>().map_err(|e| {
                        vantage_core::util::error::vantage_error!("Invalid Thing format: {}", e)
                    })?
                } else {
                    Thing::new(table.table_name(), id_str.to_string())
                };

                let record_id: surreal_client::RecordId = thing.into();
                let client = self.inner.lock().await;
                client
                    .update_record(record_id, value)
                    .await
                    .context("Failed to update record using value callback")?;
            }
        }

        Ok(())
    }

    async fn get_count<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
    ) -> vantage_dataset::dataset::Result<i64>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        use vantage_expressions::QuerySource;

        let select = table.select();
        let count_select = select.as_count();
        let count_expr: vantage_expressions::Expression = count_select.into();
        let result = self.execute(&count_expr).await;

        // Extract count from result
        if let Some(count_val) = result
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|obj| obj.as_object())
            .and_then(|map| map.get("count"))
        {
            if let Some(num) = count_val.as_i64() {
                return Ok(num);
            } else if let Some(num) = count_val.as_u64() {
                return Ok(num as i64);
            }
        }

        Ok(0)
    }

    async fn get_sum<E>(
        &self,
        table: &vantage_table::Table<Self, E>,
        column: &Self::Column,
    ) -> vantage_dataset::dataset::Result<i64>
    where
        E: vantage_core::Entity,
        Self: Sized,
    {
        use vantage_expressions::QuerySource;

        let select = table.select();
        let column_expr = column.expr();
        let sum_select = select.as_sum(column_expr);
        let sum_expr: vantage_expressions::Expression = sum_select.into();
        let result = self.execute(&sum_expr).await;

        // Extract sum from result
        if let Some(sum_val) = result
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|obj| obj.as_object())
            .and_then(|map| map.get("sum"))
        {
            if let Some(num) = sum_val.as_i64() {
                return Ok(num);
            } else if let Some(num) = sum_val.as_u64() {
                return Ok(num as i64);
            } else if let Some(num) = sum_val.as_f64() {
                return Ok(num as i64);
            }
        }

        Ok(0)
    }
}
