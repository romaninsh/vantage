use crate::{SurrealDB, SurrealInsert, thing::Thing};
use async_trait::async_trait;
use vantage_core::{error, util::error::Context};
use vantage_expressions::{Expression, expr};
use vantage_table::{ColumnLike, Table};

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
        use vantage_table::ColumnCollectionExt;
        use vantage_table::ColumnFlag;

        // Filter columns by Searchable flag
        let searchable_columns = table.columns().only(ColumnFlag::Searchable);

        if searchable_columns.is_empty() {
            // No searchable columns, return always-true expression
            return Expression::new("true", vec![]);
        }

        // Build search conditions for each searchable column using @@ operator
        let conditions: Vec<Expression> = searchable_columns
            .iter()
            .map(|(_col_name, col)| expr!("{} @@ {}", (col.expr()), search_value))
            .collect();

        // Combine all conditions with OR
        Expression::from_vec(conditions, " OR ")
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
        let mut insert = SurrealInsert::new(table.table_name()).with_id(id);

        // Use table columns to set fields with proper type conversion
        let columns = table.columns();
        for (column_name, column) in columns {
            // Use column type information to set field appropriately
            if let Some(value) = record.get(column_name) {
                match column.get_type() {
                    "string" => {
                        if let Some(s) = value.as_str() {
                            insert = insert.set_field(column_name, s.to_string());
                        }
                    }
                    "int" => {
                        if let Some(i) = value.as_i64() {
                            insert = insert.set_field(column_name, i);
                        }
                    }
                    "float" => {
                        if let Some(f) = value.as_f64() {
                            insert = insert.set_field(column_name, f);
                        }
                    }
                    "bool" => {
                        if let Some(b) = value.as_bool() {
                            insert = insert.set_field(column_name, b);
                        }
                    }
                    "decimal" => {
                        #[cfg(feature = "decimal")]
                        {
                            if let Some(s) = value.as_str() {
                                if let Ok(decimal) = s.parse::<rust_decimal::Decimal>() {
                                    insert = insert.set_field(column_name, decimal);
                                    continue;
                                }
                            }
                            if let Some(f) = value.as_f64() {
                                if let Ok(decimal) = rust_decimal::Decimal::try_from(f) {
                                    insert = insert.set_field(column_name, decimal);
                                    continue;
                                }
                            }
                        }
                        // Fallback to string
                        if let Some(s) = value.as_str() {
                            insert = insert.set_field(column_name, s.to_string());
                        }
                    }
                    _ => {
                        // For unknown types, store as JSON string
                        insert = insert.set_field(column_name, value.to_string());
                    }
                }
            }
        }

        // Handle any fields in the record that don't have corresponding columns
        if let serde_json::Value::Object(map) = &record {
            for (key, value) in map {
                if !columns.contains_key(key) {
                    // No column definition, set field directly with JSON conversion
                    match value {
                        serde_json::Value::String(s) => {
                            insert = insert.set_field(key, s.clone());
                        }
                        serde_json::Value::Number(n) => {
                            if let Some(i) = n.as_i64() {
                                insert = insert.set_field(key, i);
                            } else if let Some(f) = n.as_f64() {
                                insert = insert.set_field(key, f);
                            }
                        }
                        serde_json::Value::Bool(b) => {
                            insert = insert.set_field(key, *b);
                        }
                        serde_json::Value::Null => {
                            insert = insert.set_field(key, surreal_client::types::Any);
                        }
                        _ => {
                            // For complex types, store as string
                            insert = insert.set_field(key, value.to_string());
                        }
                    }
                }
            }
        }

        insert
            .execute(self)
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
        let count_expr = select.as_count();
        let result = self.execute(&count_expr).await;

        // SurrealDB returns count directly as a number
        if let Some(num) = result.as_i64() {
            return Ok(num);
        } else if let Some(num) = result.as_u64() {
            return Ok(num as i64);
        } else if let Some(arr) = result.as_array()
            && let Some(first) = arr.first()
        {
            if let Some(num) = first.as_i64() {
                return Ok(num);
            } else if let Some(num) = first.as_u64() {
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
        let sum_expr = select.as_sum(column_expr);
        let result = self.execute(&sum_expr).await;

        // SurrealDB returns sum directly as a number
        if let Some(num) = result.as_i64() {
            return Ok(num);
        } else if let Some(num) = result.as_u64() {
            return Ok(num as i64);
        } else if let Some(num) = result.as_f64() {
            return Ok(num as i64);
        } else if let Some(arr) = result.as_array()
            && let Some(first) = arr.first()
        {
            if let Some(num) = first.as_i64() {
                return Ok(num);
            } else if let Some(num) = first.as_u64() {
                return Ok(num as i64);
            } else if let Some(num) = first.as_f64() {
                return Ok(num as i64);
            }
        }

        Ok(0)
    }
}
