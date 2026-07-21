use vantage_core::{Result, error};
use vantage_expressions::traits::selectable::Selectable;
use vantage_expressions::{Expression, Expressive, SelectableDataSource, expr_any};
use vantage_types::{EmptyEntity, Entity};

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
            // With an active-column set, project only its members. The id
            // column is always projected — consumers rely on it to key rows.
            if !self.is_active(column.name()) {
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
            if !self.columns.contains_key(name) && self.is_active(name) {
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
        Some(self.select_expression(self.get_column_expr(field)?))
    }

    /// Wrap an arbitrary expression as a single-column subquery over this
    /// table's source and conditions: `(SELECT <expr> FROM table WHERE …)`.
    ///
    /// Extracted from [`select_column`](Self::select_column) so a traversal
    /// expression can nest one subquery inside another (multi-hop implicit
    /// references). Fields and ordering are cleared — only `expr` is projected.
    pub fn select_expression(&self, expr: Expression<T::Value>) -> Expression<T::Value>
    where
        T::Select: Expressive<T::Value>,
    {
        let mut select = self.select_empty();
        select.clear_fields();
        select.clear_order_by();
        select.add_expression(expr);
        select.expr()
    }

    /// Whether `name` is projected by [`select`](Self::select). With no active
    /// set every column is active; otherwise only the set's members are, plus
    /// the id column (always projected — consumers key rows by it).
    fn is_active(&self, name: &str) -> bool {
        match &self.active_columns {
            None => true,
            Some(set) => set.contains(name) || self.id_field.as_deref() == Some(name),
        }
    }

    /// Restrict this table to an explicit set of columns, and import **implicit
    /// references** — dotted names that traverse declared `has_one` relations
    /// and surface the target's field as a read-only, typed column aliased
    /// under the literal dotted name.
    ///
    /// ```rust,ignore
    /// let orders = Order::sqlite_table(db)
    ///     .with_active_columns(&["id", "client.name", "client.bakery.name"])?;
    /// // SELECT id,
    /// //   (SELECT name FROM client WHERE client.id = client_order.client_id) AS "client.name",
    /// //   (SELECT (SELECT name FROM bakery WHERE bakery.id = client.bakery_id)
    /// //      FROM client WHERE client.id = client_order.client_id)           AS "client.bakery.name"
    /// // FROM client_order
    /// ```
    ///
    /// A non-dotted entry restricts projection to an existing column or
    /// expression column (exactly today's declared set). A dotted entry `a.b…c`
    /// resolves `a`, `b`… as `has_one` relations and `c` as a column on the
    /// final target. Everything is validated here, so every failure is a
    /// **build-time** error, never a fetch-time surprise: unknown column,
    /// unknown relation, a `has_many` hop, or a backend that cannot lower
    /// traversal into its query (e.g. MongoDB, CSV, REST). Same-datasource
    /// only; cross-datasource traversal is a Diorama augmentation concern.
    pub fn with_active_columns(mut self, cols: &[&str]) -> Result<Self>
    where
        T: 'static,
        E: 'static,
        T::Column<T::AnyType>: Expressive<T::Value>,
        T::Select: Expressive<T::Value>,
    {
        for &col in cols {
            let parts: Vec<&str> = col.split('.').collect();
            if parts.iter().any(|p| p.is_empty()) {
                return Err(error!("invalid active column name", column = col));
            }

            if parts.len() >= 2 {
                let column = parts[parts.len() - 1];
                let hops = &parts[..parts.len() - 1];

                if !self.data_source().supports_traversal() {
                    return Err(error!(
                        "backend does not support implicit-reference traversal in columns",
                        column = col
                    ));
                }

                // Validate the has_one chain and that the final column exists.
                let (target, fk_hops) = self.resolve_has_one_target(hops)?;
                if !target.columns().contains_key(column) {
                    return Err(error!(
                        "implicit reference target has no such column",
                        column = column
                    ));
                }

                // Lower to an expression: native path first, else the generic
                // nested correlated-subquery chain. The native path receives
                // the foreign-key/link *fields*, not the relation names — a
                // SurrealDB idiom path traverses record-link fields, and a
                // relation is free to be named differently from its FK
                // (`with_one("owner", "client", …)` must lower to
                // `client.name`, not the nonexistent `owner.name`).
                let fk_refs: Vec<&str> = fk_hops.iter().map(String::as_str).collect();
                let expr = match self.data_source().traversal_path_expr(&fk_refs, column) {
                    Some(e) => e,
                    None => self.traverse_rest_generic(hops, column)?,
                };

                let dotted = col.to_string();
                if !self.columns.contains_key(&dotted) {
                    let column_def = self.data_source.create_column::<T::AnyType>(&dotted);
                    self.add_column(column_def);
                }
                self = self.with_expression(&dotted, move |_| expr.clone());
                self.imported_columns.insert(dotted.clone());
                self.active_columns
                    .get_or_insert_with(Default::default)
                    .insert(dotted);
            } else {
                // Expression columns registered via `with_expression` alone
                // (no column def) are projectable too — activating them must
                // work, or an active set would silently drop them for good.
                if !self.columns.contains_key(col) && !self.expressions.contains_key(col) {
                    return Err(error!("unknown active column", column = col));
                }
                self.active_columns
                    .get_or_insert_with(Default::default)
                    .insert(col.to_string());
            }
        }
        Ok(self)
    }

    /// Recursively lower a dotted implicit reference into nested correlated
    /// subqueries. One hop wraps the recursion's inner expression in a
    /// `get_subquery_as` target via [`select_expression`](Self::select_expression);
    /// the base case projects the final column. Used only when the backend has
    /// no native [`traversal_path_expr`](crate::prelude::TableSource::traversal_path_expr).
    fn traverse_rest_generic(&self, hops: &[&str], column: &str) -> Result<Expression<T::Value>>
    where
        T: 'static,
        E: 'static,
        T::Column<T::AnyType>: Expressive<T::Value>,
        T::Select: Expressive<T::Value>,
    {
        match hops.split_first() {
            None => self.get_column_expr(column).ok_or_else(|| {
                error!(
                    "implicit reference target has no such column",
                    column = column
                )
            }),
            Some((head, tail)) => {
                let target: Table<T, EmptyEntity> = self.get_subquery_erased(head)?;
                let inner = target.traverse_rest_generic(tail, column)?;
                // The base case returns a bare column; a deeper hop returns a
                // SELECT that must be parenthesized before it can nest as a
                // scalar inside this hop's SELECT.
                let inner = if tail.is_empty() {
                    inner
                } else {
                    expr_any!("({})", (inner))
                };
                Ok(target.select_expression(inner))
            }
        }
    }

    /// Walk a chain of `has_one` hops and return the final target table along
    /// with each hop's foreign-key/link field (in hop order), erroring at
    /// build time on an unknown relation or a `has_many` hop. The FK fields
    /// feed the backend-native path lowering, which traverses fields — the
    /// relation *names* only address the refs registry.
    fn resolve_has_one_target(&self, hops: &[&str]) -> Result<(Table<T, EmptyEntity>, Vec<String>)>
    where
        T: 'static,
        E: 'static,
    {
        let (head, tail) = hops
            .split_first()
            .ok_or_else(|| error!("empty implicit reference path"))?;
        if self.ref_cardinality(head)? != vantage_vista::ReferenceKind::HasOne {
            return Err(error!(
                "implicit reference hop must traverse a has_one relation",
                relation = *head
            ));
        }
        let fk = self.ref_foreign_key(head)?;
        let target: Table<T, EmptyEntity> = self.get_ref_target_erased(head)?;
        if tail.is_empty() {
            Ok((target, vec![fk]))
        } else {
            let (final_target, mut fks) = target.resolve_has_one_target(tail)?;
            fks.insert(0, fk);
            Ok((final_target, fks))
        }
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
    ///
    /// Implicit references are **not** inherited: the derived table starts with
    /// no active set, and listing an imported dotted column in `columns` copies
    /// only its bare definition (no traversal expression, no read-only
    /// tracking). Re-declare traversals on the derived table if needed.
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
