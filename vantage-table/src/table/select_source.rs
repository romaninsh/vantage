impl<T, E> Table<T, E>
where
    T: TableSource + SelectSource<T::Value>,
    E: Entity,
{
    /// Get data from the table using the configured columns and conditions
    pub async fn get(&self) -> Result<Vec<E>> {
        // Use TableSource directly instead of QuerySource
        let entities_with_ids = self
            .data_source
            .get_table_data(self)
            .await
            .with_context(|| error!("Failed to get table data"))?;
        Ok(entities_with_ids
            .into_iter()
            .map(|(_, entity)| entity)
            .collect())
    }

    /// Get raw data from the table as `Vec<Value>` without entity deserialization
    pub async fn get_values(&self) -> Result<Vec<serde_json::Value>>
    where
        T: QuerySource<T::Value>,
        T::Value: Into<serde_json::Value>,
    {
        let select = self.select();
        let raw_result = self.data_source.execute(&select.into()).await;
        let json_value: serde_json::Value = raw_result.into();

        // Try to parse as array of objects
        if let serde_json::Value::Array(items) = json_value {
            Ok(items)
        } else {
            Err(vantage_error!("Expected array of objects from database"))
        }
    }

    /// Get count of records in the table
    pub async fn get_count(&self) -> Result<i64> {
        self.data_source.get_count(self).await
    }

    /// Get sum of a column in the table
    pub async fn get_sum(&self, column: &T::Column) -> Result<i64> {
        self.data_source.get_sum(self, column).await
    }

    /// Create a count query expression (does not execute)
    pub fn get_count_query(&self) -> Expression<T::Value> {
        self.select().as_count()
    }

    /// Create a sum query expression for a column (does not execute)
    pub fn get_sum_query(&self, column: &T::Column) -> Expression<T::Value> {
        self.select().as_sum(column.expr())
    }

    /// Create a select query with table configuration applied
    pub fn select(&self) -> T::Select {
        let mut select = self.data_source.select();

        // Set the table as source
        select.set_source(self.table_name.as_str(), None);

        // Add all columns as fields
        for column in self.columns.values() {
            match column.alias() {
                Some(alias) => select.add_expression(column.expr(), Some(alias.to_string())),
                None => select.add_field(column.name()),
            }
        }

        // Add all conditions
        for condition in self.conditions.values() {
            select.add_where_condition(condition.clone());
        }

        // Add all order clauses
        for (expr, direction) in self.order_by.values() {
            let ascending = matches!(direction, crate::with_ordering::SortDirection::Ascending);
            select.add_order_by(expr.clone(), ascending);
        }

        // Apply pagination
        if let Some(pagination) = &self.pagination {
            pagination.apply_on_select(&mut select);
        }

        select
    }
}
