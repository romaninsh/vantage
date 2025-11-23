use vantage_core::Result;
use vantage_expressions::{Expression, Expressive, SelectSource, Selectable};
use vantage_types::Entity;

use crate::{table::Table, traits::column_like::ColumnLike, traits::table_source::TableSource};

impl<T, E> Table<T, E>
where
    T: SelectSource<T::Value> + TableSource,
    T::Select: Selectable<T::Value>,
    T::Value: From<String>, // that's because table is specified as a string
    T::Column: Expressive<T::Value>,
    E: Entity<T::Value>,
{
    /// Create a select query with table configuration applied
    pub fn select(&self) -> T::Select {
        let mut select = self.data_source.select();

        // Set the table as source
        select.set_source(self.table_name(), None);

        // Add all columns as fields
        for column in self.columns.values() {
            match column.alias() {
                Some(alias) => select.add_expression(
                    self.data_source.expr(column.name(), vec![]),
                    Some(alias.to_string()),
                ),
                None => select.add_field(column.name()),
            }
        }

        // Add all conditions
        for condition in self.conditions.values() {
            select.add_where_condition(condition.clone());
        }

        // Add all order clauses
        for (expr, direction) in self.order_by.values() {
            let ascending = matches!(direction, crate::sorting::SortDirection::Ascending);
            select.add_order_by(expr.clone(), ascending);
        }

        // Apply pagination
        if let Some(pagination) = &self.pagination {
            select.set_limit(Some(pagination.limit()), Some(pagination.skip()));
        }

        select
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
}

// Specific implementation for serde_json::Value that can use QuerySource
impl<T, E> Table<T, E>
where
    T: SelectSource<serde_json::Value>
        + TableSource<Value = serde_json::Value>
        + vantage_expressions::traits::datasource::QuerySource<serde_json::Value>,
    T::Select: Selectable<serde_json::Value>,
    T::Column: Expressive<serde_json::Value>,
    E: Entity<serde_json::Value>,
{
    /// Get count using QuerySource for serde_json::Value
    pub async fn get_count_via_query(&self) -> Result<i64> {
        let count_query = self.get_count_query();
        let result = self.data_source.execute(&count_query).await?;

        // Extract count from result - could be {"count": 42} or just 42
        if let Some(count) = result.get("count").and_then(|v| v.as_i64()) {
            Ok(count)
        } else if let Some(count) = result.as_i64() {
            Ok(count)
        } else {
            Ok(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::tablesource::MockTableSource;
    use serde_json::json;
    use vantage_expressions::mocks::datasource::{MockQuerySource, MockSelectSource};
    use vantage_expressions::traits::datasource::QuerySource;

    #[tokio::test]
    async fn test_selectable_functionality() {
        let mock_select_source = MockSelectSource::new(json!([
            {"id": "1", "name": "Alice", "age": 30},
            {"id": "2", "name": "Bob", "age": 25}
        ]));

        let mock_query_source = MockQuerySource::new(json!({"count": 42}));

        let table = MockTableSource::new()
            .with_data(
                "users",
                vec![
                    json!({"id": "1", "name": "Alice", "age": 30}),
                    json!({"id": "2", "name": "Bob", "age": 25}),
                ],
            )
            .with_select_source(mock_select_source)
            .with_query_source(mock_query_source);
        let table = Table::<_, vantage_types::EmptyEntity>::new("users", table);

        // Basic select
        let select = table.select();
        assert_eq!(select.source(), Some("users"));

        // Validate SQL query generation
        let query_expr: vantage_expressions::Expression<serde_json::Value> = select.into();
        assert_eq!(query_expr.preview(), "SELECT * FROM users");

        // Test count query generation
        let count_query = table.get_count_query();
        assert_eq!(count_query.preview(), "SELECT COUNT(*) FROM \"users\"");

        // Test sum query generation
        let age_column = table.data_source().create_column("age", table.clone());
        let sum_query = table.get_sum_query(&age_column);
        assert_eq!(sum_query.preview(), "SELECT SUM(age) FROM \"users\"");

        // Test actual count/sum methods - get_count should return 42 from mock query source
        let count = table.get_count_via_query().await.unwrap();
        assert_eq!(count, 42);
    }

    #[tokio::test]
    #[should_panic(expected = "MockTableSource select source not set")]
    async fn test_panics_without_select_source() {
        let table = Table::<_, vantage_types::EmptyEntity>::new("users", MockTableSource::new());
        let _select = table.select();
    }

    #[tokio::test]
    #[should_panic(expected = "MockTableSource query source not set")]
    async fn test_panics_without_query_source() {
        let table = Table::<_, vantage_types::EmptyEntity>::new("users", MockTableSource::new());
        let query = table.data_source().expr("SELECT COUNT(*)", vec![]);
        let _result = table.data_source().execute(&query).await;
    }
}
