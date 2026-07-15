use vantage_core::Result;
use vantage_expressions::traits::selectable::Selectable;
use vantage_expressions::{Expression, Expressive, SelectableDataSource, expr_any};
use vantage_types::Entity;

use crate::{
    column::core::ColumnType,
    source::{SelectSeed, SelectSource},
    table::Table,
    traits::column_like::ColumnLike,
    traits::table_source::TableSource,
};

impl<T, E> Table<T, E>
where
    T: SelectableDataSource<T::Value, T::Condition> + TableSource,
    T::Source: SelectSeed<T::Select, T::Value, T::Condition>,
    T::Value: From<String>, // that's because table is specified as a string
    E: Entity<T::Value>,
{
    /// Create a bare select with source, conditions, ordering, and pagination —
    /// but no fields. Used by `select_column` and aggregates to avoid evaluating
    /// all expressions.
    pub fn select_empty(&self) -> T::Select {
        let mut select = self.data_source.select();
        self.source.seed(&mut select);

        for condition in self.conditions.values() {
            select.add_where_condition(condition.clone());
        }

        for (expr, direction) in self.order_by.values() {
            let order = match direction {
                crate::sorting::SortDirection::Ascending => vantage_expressions::Order::Asc,
                crate::sorting::SortDirection::Descending => vantage_expressions::Order::Desc,
            };
            select.add_order_by(expr.clone(), order);
        }

        if let Some(pagination) = &self.pagination {
            select.set_limit(Some(pagination.limit()), Some(pagination.skip()));
        }

        select
    }

    /// Create a select query with table configuration applied
    pub fn select(&self) -> T::Select {
        let mut select = self.select_empty();

        // Add all columns as fields (or expressions if defined)
        for column in self.columns.values() {
            // Lazy-expression columns exist only on returned records — the
            // source has no such field to project (SQLite would silently
            // degrade the unknown quoted identifier to a string literal;
            // other SQL backends would error).
            if self.lazy_expressions.contains_key(column.name()) {
                continue;
            }
            if let Some(expr_fn) = self.expressions.get(column.name()) {
                let expr = expr_fn(self.as_entity_erased());
                self.data_source.add_select_column(
                    &mut select,
                    expr_any!("({})", (expr)),
                    Some(column.name()),
                );
            } else if let Some(alias) = column.alias() {
                let expr = self.data_source.expr(column.name(), vec![]);
                self.data_source
                    .add_select_column(&mut select, expr, Some(alias));
            } else {
                select.add_field(column.name());
            }
        }

        // Add expressions that don't correspond to any column
        for (name, expr_fn) in &self.expressions {
            if !self.columns.contains_key(name) {
                let expr = expr_fn(self.as_entity_erased());
                self.data_source.add_select_column(
                    &mut select,
                    expr_any!("({})", (expr)),
                    Some(name),
                );
            }
        }

        select
    }
    /// Get count of records in the table
    pub async fn get_count(&self) -> Result<i64> {
        self.data_source.get_table_count(self).await
    }

    /// Get sum of a column in the table
    pub async fn get_sum(&self, column: &T::Column<T::AnyType>) -> Result<T::Value> {
        self.data_source.get_table_sum(self, column).await
    }

    /// Get max of a column in the table
    pub async fn get_max(&self, column: &T::Column<T::AnyType>) -> Result<T::Value> {
        self.data_source.get_table_max(self, column).await
    }

    /// Get min of a column in the table
    pub async fn get_min(&self, column: &T::Column<T::AnyType>) -> Result<T::Value> {
        self.data_source.get_table_min(self, column).await
    }

    /// Create a count query expression (does not execute).
    /// The result is wrapped in parentheses so it's safe to nest as a subquery.
    pub fn get_count_query(&self) -> Expression<T::Value> {
        expr_any!("({})", (self.select_empty().as_count()))
    }

    /// Create a sum query expression for a column (does not execute).
    /// The result is wrapped in parentheses so it's safe to nest as a subquery.
    pub fn get_sum_query<Type>(&self, column: &T::Column<Type>) -> Expression<T::Value>
    where
        Type: ColumnType,
        T::Column<Type>: Expressive<T::Value>,
    {
        expr_any!("({})", (self.select_empty().as_sum(column.expr())))
    }

    /// Create a subquery expression that selects a single column from this table.
    ///
    /// Builds `SELECT field FROM table WHERE conditions` — useful as a correlated
    /// subquery inside `with_expression`. Returns `None` if `field` is not a
    /// column on this table (mirroring [`get_column_expr`](Self::get_column_expr));
    /// when the caller hardcodes a known column name, `.expect(...)` is fine:
    ///
    /// ```rust,ignore
    /// .with_expression("category", |t| {
    ///     t.get_subquery_as::<Category>("category").unwrap()
    ///         .select_column("name")
    ///         .expect("Category has a 'name' column")
    /// })
    /// ```
    pub fn select_column(&self, field: &str) -> Option<Expression<T::Value>>
    where
        T::Column<T::AnyType>: Expressive<T::Value>,
        T::Select: Expressive<T::Value>,
    {
        let expr = self.get_column_expr(field)?;
        let mut select = self.select_empty();
        select.clear_fields();
        select.clear_order_by();
        select.add_expression(expr);
        Some(select.expr())
    }
}

// Constructors for tables sourced from an arbitrary query (a derived / sub-SELECT
// source). Only available to backends whose `Source` is `SelectSource<Select>`
// (the four subquery-capable SQL/SurrealDB backends). `V`/`C`/`S` are named
// explicitly rather than projected through `T` to avoid a bound-resolution cycle.
impl<T, E, V, C, S> Table<T, E>
where
    T: SelectableDataSource<V, C, Select = S>
        + TableSource<Value = V, Condition = C, Source = SelectSource<S>>,
    V: Clone + Send + Sync + 'static + From<String>,
    C: Clone + Send + Sync + 'static,
    S: Expressive<V> + Clone,
    E: Entity<V>,
{
    /// Build a read-only table whose FROM clause is `select`, exposed under
    /// `alias`. Columns/relations start empty — declare or inherit them.
    pub fn from_select(data_source: T, alias: impl Into<String>, select: S) -> Self {
        let alias = alias.into();
        let mut table = Table::new(alias.clone(), data_source);
        table.source = SelectSource::query(select, alias);
        table
    }

    /// Derive a table from an existing one: transform its select via `modifier`
    /// and use the result as the (sub-SELECT) source, inheriting the listed
    /// `columns` and `relations` plus identity/title metadata.
    ///
    /// `modifier` receives `source.select()` and decides flat-vs-wrapped — it
    /// may extend the query in place (joins referencing the base tables) or wrap
    /// it as a subquery (to filter/sort on a computed alias). Conditions already
    /// baked into the base select are not re-applied.
    pub fn derive_from<E2: Entity<V> + 'static>(
        source: &Table<T, E2>,
        alias: impl Into<String>,
        modifier: impl FnOnce(S) -> S,
        columns: &[&str],
        relations: &[&str],
    ) -> Self
    where
        T: 'static,
        E: 'static,
    {
        let alias = alias.into();
        let select = modifier(source.select());
        let mut table = Table::new(alias.clone(), source.data_source().clone());
        table.source = SelectSource::query(select, alias);
        table.copy_columns_from(source, Some(columns));
        table.copy_relations_from(source, Some(relations));
        table.id_field = source.id_field.clone();
        table.title_field = source.title_field.clone();
        table.title_fields = source.title_fields.clone();
        table
    }
}

// Specific implementation for serde_json::Value that can use QuerySource
impl<T, E> Table<T, E>
where
    T: SelectableDataSource<serde_json::Value, T::Condition>
        + TableSource<Value = serde_json::Value>
        + vantage_expressions::traits::datasource::ExprDataSource<serde_json::Value>,
    T::Source: SelectSeed<T::Select, serde_json::Value, T::Condition>,
    T::Value: From<String>,
    E: Entity<serde_json::Value>,
{
    /// Get count using QuerySource for serde_json::Value
    pub async fn get_count_via_query(&self) -> Result<i64> {
        let count_query = self.get_count_query();
        let result = self.data_source.execute(&count_query).await?;

        // Unwrap a single-element array, e.g. `[{"count": 42}]` or `[42]`,
        // which is how SQL/Surreal count queries commonly come back.
        let result = match result.as_array().map(Vec::as_slice) {
            Some([single]) => single,
            _ => &result,
        };

        // Extract count from result - could be {"count": 42} or just 42.
        // Anything else is an unexpected shape: surface it rather than
        // silently reporting zero rows.
        if let Some(count) = result.get("count").and_then(|v| v.as_i64()) {
            Ok(count)
        } else if let Some(count) = result.as_i64() {
            Ok(count)
        } else {
            Err(vantage_core::util::error::vantage_error!(
                "count query returned an unexpected result shape: {result}"
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mocks::mock_table_source::MockTableSource;
    use serde_json::json;
    use vantage_expressions::mocks::datasource::MockSelectableDataSource;
    use vantage_expressions::traits::datasource::ExprDataSource;

    #[tokio::test]
    async fn test_selectable_functionality() {
        let mock_select_source = MockSelectableDataSource::new(json!([
            {"id": "1", "name": "Alice", "age": 30},
            {"id": "2", "name": "Bob", "age": 25}
        ]));

        let mock_query_source = vantage_expressions::mocks::mock_builder::new()
            .on_exact_select("(SELECT COUNT(*) FROM \"users\")", json!(42));

        let table = MockTableSource::new()
            .with_data(
                "users",
                vec![
                    json!({"id": "1", "name": "Alice", "age": 30}),
                    json!({"id": "2", "name": "Bob", "age": 25}),
                ],
            )
            .await
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
        assert_eq!(count_query.preview(), "(SELECT COUNT(*) FROM \"users\")");

        // TODO: This does not work with MockColumn - because it does not implement Expressive
        // // Test sum query generation
        // let age_column = table.data_source().create_column::<i64>("age");
        // let sum_query = table.get_sum_query(&age_column);
        // assert_eq!(sum_query.preview(), "SELECT SUM(age) FROM \"users\"");

        // Test actual count/sum methods - get_count should return 42 from mock query source
        let count = table.get_count_via_query().await.unwrap();
        assert_eq!(count, 42);
    }

    async fn count_table_returning(
        count_result: serde_json::Value,
    ) -> Table<MockTableSource, vantage_types::EmptyEntity> {
        let mock_select_source = MockSelectableDataSource::new(json!([]));
        let mock_query_source = vantage_expressions::mocks::mock_builder::new()
            .on_exact_select("(SELECT COUNT(*) FROM \"users\")", count_result);
        let source = MockTableSource::new()
            .with_select_source(mock_select_source)
            .with_query_source(mock_query_source);
        Table::<_, vantage_types::EmptyEntity>::new("users", source)
    }

    #[tokio::test]
    async fn test_count_unwraps_single_element_array() {
        // SQL/Surreal count queries commonly return `[{"count": N}]`.
        let table = count_table_returning(json!([{"count": 7}])).await;
        assert_eq!(table.get_count_via_query().await.unwrap(), 7);
    }

    #[tokio::test]
    async fn test_count_errors_on_unexpected_shape() {
        // An unrecognized result must surface as an error, not a silent zero.
        let table = count_table_returning(json!({"total": 5})).await;
        assert!(table.get_count_via_query().await.is_err());
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
