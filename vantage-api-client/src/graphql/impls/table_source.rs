//! `TableSource` for `GraphqlApi`.
//!
//! Bridges `Table<GraphqlApi, E>` to `GraphqlSelect`-driven HTTP requests.
//! Each call builds a select from the table's current state (columns →
//! selection set, conditions → filter arg, orders → `order_by`, pagination
//! → `$limit/$offset` variables), runs it, and reshapes the response.
//!
//! v1 covers reads + count. Writes (insert/update/delete) and SQL-style
//! aggregates (sum/max/min) return `unimplemented!` errors until a real
//! consumer drives their shape (Hasura mutations vs hand-rolled
//! `createUser` vs Postgraphile `userCreate` all differ significantly).

use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::Value;
use vantage_core::error;
use vantage_dataset::ReadableValueSet;
use vantage_dataset::traits::Result;
use vantage_expressions::{
    AssociatedExpression, DeferredFn, ExprDataSource, Expression, Order,
    traits::expressive::ExpressiveEnum,
};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::graphql::api::GraphqlApi;
use crate::graphql::condition::{GraphqlCondition, GraphqlOp};
use crate::graphql::operation::GraphqlOperation;
use crate::graphql::select::GraphqlSelect;
use crate::graphql::types::AnyGraphqlType;

/// Build a `GraphqlSelect` from a table's current state.
///
/// - `root_field` ← `table.table_name()`
/// - `fields` ← `table.columns().keys()` (plus id field if not in the column set)
/// - `conditions` ← `table.conditions()`
/// - `sort` ← `table.orders()`, mapping the condition's first `Field` to a column name (mirrors Mongo's posture)
/// - `limit/skip` ← `table.pagination()`
/// - `dialect` / `filter_arg_name` propagate from the API
fn select_from_table<E: Entity<AnyGraphqlType>>(table: &Table<GraphqlApi, E>) -> GraphqlSelect {
    let api = table.data_source();
    let mut select = GraphqlSelect::new()
        .with_root_field(table.table_name())
        .with_dialect(api.dialect);

    if let Some(name) = api.filter_arg_name.clone() {
        select = select.with_filter_arg_name(name);
    }

    // Selection set
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let id_name = table.id_field().map(|c| c.name().to_string());
    if let Some(id_name) = id_name.as_ref() {
        select = select.with_field(id_name);
        seen.insert(id_name.clone());
    }
    for (name, _col) in table.columns() {
        if seen.insert(name.clone()) {
            select = select.with_field(name);
        }
    }

    // Conditions
    for cond in table.conditions() {
        select.conditions.push(cond.clone());
    }

    // Orders: GraphqlCondition's `Field` variant carries the column name.
    for (cond, direction) in table.orders() {
        if let GraphqlCondition::Field(fc) = cond {
            let order = if matches!(direction, vantage_table::sorting::SortDirection::Ascending) {
                Order::Asc
            } else {
                Order::Desc
            };
            select.sort.push((fc.field.clone(), order));
        }
    }

    // Pagination
    if let Some(pagination) = table.pagination() {
        select.limit = Some(pagination.limit());
        select.skip = Some(pagination.skip());
    }

    select
}

/// Convert a JSON row into `(Id, Record<AnyGraphqlType>)`. The id is
/// stringified from whatever JSON shape the server returned — most
/// schemas use `String` or numeric ids, both of which we coerce to
/// `String` since that's our `Id` type.
fn row_to_record(row: &Value, id_field: Option<&str>) -> Result<(String, Record<AnyGraphqlType>)> {
    let obj = row
        .as_object()
        .ok_or_else(|| error!("Expected JSON object for row", got = format!("{:?}", row)))?;

    let id = match id_field {
        Some(field) => obj
            .get(field)
            .map(value_to_string)
            .ok_or_else(|| error!("Row missing id field", field = field.to_string()))?,
        // No id field declared — fall back to "id" then to a stringified row index later.
        None => obj.get("id").map(value_to_string).unwrap_or_default(),
    };

    let mut fields: IndexMap<String, AnyGraphqlType> = IndexMap::new();
    for (k, v) in obj {
        fields.insert(k.clone(), AnyGraphqlType::untyped(v.clone()));
    }

    Ok((id, Record::from_indexmap(fields)))
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[async_trait]
impl TableSource for GraphqlApi {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = AnyGraphqlType;
    type Value = AnyGraphqlType;
    type Id = String;
    type Condition = GraphqlCondition;
    type Source = String;

    /// Stringy `field == value` helper for callers that only have text
    /// on hand (CLI, generic UIs). The value lands as a JSON string.
    fn eq_condition(field: &str, value: &str) -> Result<Self::Condition> {
        Ok(Column::<AnyGraphqlType>::new(field).eq(value))
    }

    /// Typed-value sibling of `eq_condition`. Used by
    /// `Reference::resolve_from_row` to push a row-derived `AnyGraphqlType`
    /// join value onto a child table without a string round-trip.
    fn eq_value_condition(&self, field: &str, value: Self::Value) -> Result<Self::Condition> {
        Ok(Column::<AnyGraphqlType>::new(field).eq(value))
    }

    fn create_column<Type: ColumnType>(&self, name: &str) -> Self::Column<Type> {
        Column::new(name)
    }

    fn to_any_column<Type: ColumnType>(
        &self,
        column: Self::Column<Type>,
    ) -> Self::Column<Self::AnyType> {
        Column::from_column(column)
    }

    fn convert_any_column<Type: ColumnType>(
        &self,
        any_column: Self::Column<Self::AnyType>,
    ) -> Option<Self::Column<Type>> {
        Some(Column::from_column(any_column))
    }

    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<ExpressiveEnum<Self::Value>>,
    ) -> Expression<Self::Value> {
        Expression::new(template, parameters)
    }

    /// Build an OR-of-ILIKEs across all of the table's columns — the
    /// closest analogue to a `SEARCH 'value'` operator that survives
    /// across servers. Hasura speaks `_ilike` natively; Generic dialect
    /// rejects this at render time, which is the right failure mode
    /// (search doesn't translate to flat-arg schemas).
    fn search_table_condition<E>(
        &self,
        table: &Table<Self, E>,
        search_value: &str,
    ) -> Self::Condition
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let pattern = format!("%{}%", search_value);
        let conditions: Vec<GraphqlCondition> = table
            .columns()
            .keys()
            .map(|name| Column::<AnyGraphqlType>::new(name).ilike(pattern.clone()))
            .collect();

        match conditions.len() {
            0 => Column::<AnyGraphqlType>::new("__never__").eq(false),
            1 => conditions.into_iter().next().unwrap(),
            _ => GraphqlCondition::Or(conditions),
        }
    }

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let select = select_from_table(table);
        let rendered = select.render().await?;
        let data = self
            .post_graphql(&rendered.query, &rendered.variables)
            .await?;

        let root = table.table_name();
        let rows = data.get(root).ok_or_else(|| {
            error!(
                "GraphQL response missing root field",
                field = root.to_string()
            )
        })?;

        let arr = match rows {
            Value::Array(a) => a.clone(),
            Value::Null => Vec::new(),
            other => {
                return Err(error!(
                    "Unexpected response shape — expected array under root field",
                    root = root.to_string(),
                    got = format!("{:?}", other)
                ));
            }
        };

        let id_name = table.id_field().map(|c| c.name().to_string());
        let mut out = IndexMap::with_capacity(arr.len());
        for (idx, row) in arr.iter().enumerate() {
            let (mut id, rec) = row_to_record(row, id_name.as_deref())?;
            if id.is_empty() {
                id = idx.to_string();
            }
            out.insert(id, rec);
        }
        Ok(out)
    }

    /// Single-row fetch by id. Adds an `id = <id>` condition to the
    /// table's existing filter and pulls the first row.
    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Option<Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let id_name = table
            .id_field()
            .map(|c| c.name().to_string())
            .unwrap_or_else(|| "id".to_string());
        let mut select = select_from_table(table);
        select
            .conditions
            .push(Column::<AnyGraphqlType>::new(&id_name).eq(id.as_str()));
        select.limit = Some(1);

        let rendered = select.render().await?;
        let data = self
            .post_graphql(&rendered.query, &rendered.variables)
            .await?;
        let root = table.table_name();
        let rows = data.get(root).ok_or_else(|| {
            error!(
                "GraphQL response missing root field",
                field = root.to_string()
            )
        })?;

        let arr = match rows {
            Value::Array(a) => a.clone(),
            Value::Null => return Ok(None),
            // Some `byId`-style root fields return a single object instead of an array.
            Value::Object(_) => vec![rows.clone()],
            other => {
                return Err(error!(
                    "Unexpected response shape for get",
                    got = format!("{:?}", other)
                ));
            }
        };
        match arr.into_iter().next() {
            Some(row) => {
                let (_id, rec) = row_to_record(&row, Some(&id_name))?;
                Ok(Some(rec))
            }
            None => Ok(None),
        }
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let records = self.list_table_values(table).await?;
        Ok(records.into_iter().next())
    }

    /// Best-effort count — lists rows and counts them. Hasura users can
    /// override per-table once an aggregate path lands.
    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let records = self.list_table_values(table).await?;
        Ok(records.len() as i64)
    }

    async fn get_table_sum<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("sum() not implemented for GraphqlApi"))
    }

    async fn get_table_max<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("max() not implemented for GraphqlApi"))
    }

    async fn get_table_min<E>(
        &self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Self::AnyType>,
    ) -> Result<Self::Value>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("min() not implemented for GraphqlApi"))
    }

    async fn insert_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!(
            "GraphQL mutations not implemented; depends on schema"
        ))
    }

    async fn replace_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!(
            "GraphQL mutations not implemented; depends on schema"
        ))
    }

    async fn patch_table_value<E>(
        &self,
        _table: &Table<Self, E>,
        _id: &Self::Id,
        _partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!(
            "GraphQL mutations not implemented; depends on schema"
        ))
    }

    async fn delete_table_value<E>(&self, _table: &Table<Self, E>, _id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!(
            "GraphQL mutations not implemented; depends on schema"
        ))
    }

    async fn delete_table_all_values<E>(&self, _table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!(
            "GraphQL mutations not implemented; depends on schema"
        ))
    }

    async fn insert_table_return_id_value<E>(
        &self,
        _table: &Table<Self, E>,
        _record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!(
            "GraphQL mutations not implemented; depends on schema"
        ))
    }

    /// Build a child condition for `with_many` / `with_one` traversal.
    ///
    /// Two paths, mirroring the REST adapter's posture:
    ///
    /// * **Sync peek** — if the parent already carries an eq-condition
    ///   on `source_column` (the common `with_many` case where the
    ///   source column is the parent's id field narrowed via
    ///   `eq(<id>)`), re-key the value onto `target_field` immediately
    ///   so the child filter is fully concrete.
    /// * **Deferred fallback** — otherwise (the `with_one` case where
    ///   `source_column` is a foreign-key field that lives in the
    ///   parent's data, not its conditions), wrap resolution in a
    ///   `DeferredField` that fetches the parent's first row at fetch
    ///   time and pulls `source_column` out of it.
    ///
    /// Both paths render through `FilterDialect`, so Hasura schemas get
    /// `{ target_field: { _eq: v } }` and Generic schemas get the flat
    /// `{ target_field: v }` form.
    ///
    /// This is the two-round-trip option; nested-selection (single
    /// round trip) is a future optimisation that requires going around
    /// the `Table` trait surface and is tracked separately.
    fn related_in_condition<SourceE: Entity<Self::Value> + 'static>(
        &self,
        target_field: &str,
        source_table: &Table<Self, SourceE>,
        source_column: &str,
    ) -> Self::Condition
    where
        Self: Sized,
    {
        // Sync peek: look for an existing eq-condition on the parent
        // whose field matches `source_column`. Re-key onto `target_field`
        // via the operator trait so dialect rendering and value mapping
        // stay in one place.
        for cond in source_table.conditions() {
            if let GraphqlCondition::Field(fc) = cond
                && fc.field == source_column
                && fc.op == GraphqlOp::Eq
            {
                return Column::<AnyGraphqlType>::new(target_field).eq(fc.value.clone());
            }
        }

        // Deferred fallback: list the parent's rows at fetch time and
        // pull `source_column` from the first row. Wrap as a
        // `DeferredField` so the dialect-correct render path applies.
        let parent = source_table.clone();
        let column = source_column.to_string();
        let parent_name = source_table.table_name().to_string();
        let value_fn = DeferredFn::new(move || {
            let parent = parent.clone();
            let column = column.clone();
            let parent_name = parent_name.clone();
            Box::pin(async move {
                let records = parent.list_values().await?;
                let value = records
                    .values()
                    .next()
                    .and_then(|r| r.get(&column))
                    .cloned()
                    .ok_or_else(|| {
                        error!(
                            "Deferred FK resolve: parent yielded no row or column missing",
                            table = parent_name,
                            column = column
                        )
                    })?;
                Ok(ExpressiveEnum::Scalar(value))
            })
        });

        GraphqlCondition::DeferredField {
            field: target_field.to_string(),
            op: GraphqlOp::Eq,
            value_fn,
        }
    }

    /// Defer to a query that selects just `column`. Used by the
    /// `Table::with_one` / `Table::with_many` plumbing when it wants a
    /// list of values from another table (e.g. all FK ids).
    ///
    /// We implement the deferred path via the existing `list_table_values`
    /// machinery — fetch all parent rows, extract the column. For Hasura
    /// schemas this could be replaced with a proper sub-select; for
    /// SpaceX-style generic schemas, list-and-extract is the only viable
    /// path anyway.
    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        table: &Table<Self, E>,
        column: &Self::Column<Type>,
    ) -> AssociatedExpression<'a, Self, Self::Value, Vec<Type>>
    where
        E: Entity<Self::Value> + 'static,
        Self: Sized,
    {
        use vantage_expressions::expr_any;

        let table_clone = table.clone();
        let col = column.name().to_string();
        let api = self.clone();

        let inner = expr_any!("{}", {
            DeferredFn::new(move || {
                let api = api.clone();
                let table = table_clone.clone();
                let col = col.clone();
                Box::pin(async move {
                    let records = api.list_table_values(&table).await?;
                    let values: Vec<AnyGraphqlType> = records
                        .values()
                        .filter_map(|r| r.get(&col).cloned())
                        .collect();
                    Ok(ExpressiveEnum::Scalar(AnyGraphqlType::new(values)))
                })
            })
        });

        let expr = expr_any!("{}", { self.defer(inner) });
        AssociatedExpression::new(expr, self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use vantage_types::EmptyEntity;

    use crate::graphql::condition::{FieldCondition, FilterDialect};

    fn launches_table() -> Table<GraphqlApi, EmptyEntity> {
        let api = GraphqlApi::new("https://api.test/graphql");
        Table::new("launches", api)
            .with_id_column("id")
            .with_column_of::<String>("mission_name")
            .with_column_of::<i64>("launch_year")
    }

    #[test]
    fn select_from_table_populates_root_and_fields() {
        let table = launches_table();
        let select = select_from_table(&table);
        assert_eq!(select.root_field.as_deref(), Some("launches"));
        // Id field first, then declared columns.
        assert_eq!(select.fields, vec!["id", "mission_name", "launch_year"]);
        assert!(select.conditions.is_empty());
        assert_eq!(select.dialect, FilterDialect::Generic);
    }

    #[test]
    fn select_from_table_carries_conditions() {
        let mut table = launches_table();
        table.add_condition(GraphqlCondition::Field(FieldCondition::new(
            "mission_name",
            GraphqlOp::Eq,
            json!("FalconSat"),
        )));
        let select = select_from_table(&table);
        assert_eq!(select.conditions.len(), 1);
    }

    #[tokio::test]
    async fn select_from_table_renders_with_id_field_included() {
        let table = launches_table();
        let q = select_from_table(&table).render().await.unwrap();
        assert_eq!(
            q.query,
            "query { launches { id mission_name launch_year } }"
        );
    }

    #[test]
    fn eq_condition_static_builds_string_value() {
        let cond = <GraphqlApi as TableSource>::eq_condition("name", "Alice").unwrap();
        match cond {
            GraphqlCondition::Field(fc) => {
                assert_eq!(fc.field, "name");
                assert_eq!(fc.op, GraphqlOp::Eq);
                assert_eq!(fc.value, json!("Alice"));
            }
            _ => panic!("expected Field"),
        }
    }

    #[test]
    fn search_table_condition_builds_or_of_ilikes() {
        let api = GraphqlApi::new("https://api.test/graphql");
        let table = launches_table();
        let cond = api.search_table_condition(&table, "falcon");
        match cond {
            GraphqlCondition::Or(parts) => {
                // id + mission_name + launch_year — `columns()` returns every
                // declared column including the id.
                assert_eq!(parts.len(), 3);
                for p in parts {
                    match p {
                        GraphqlCondition::Field(fc) => {
                            assert_eq!(fc.op, GraphqlOp::ILike);
                            assert_eq!(fc.value, json!("%falcon%"));
                        }
                        _ => panic!("expected Field inside Or"),
                    }
                }
            }
            _ => panic!("expected Or for multi-column search"),
        }
    }

    #[test]
    fn row_to_record_extracts_id_and_fields() {
        let row = json!({ "id": "5", "mission_name": "FalconSat", "launch_year": 2006 });
        let (id, rec) = row_to_record(&row, Some("id")).unwrap();
        assert_eq!(id, "5");
        assert_eq!(rec.iter().count(), 3);
    }

    #[test]
    fn row_to_record_stringifies_numeric_id() {
        let row = json!({ "id": 42, "name": "x" });
        let (id, _rec) = row_to_record(&row, Some("id")).unwrap();
        assert_eq!(id, "42");
    }

    #[test]
    fn related_in_condition_sync_peeks_parent_eq() {
        let mut parent = launches_table();
        // Parent narrowed to id=5; child should get its target field
        // bound to 5 immediately (no deferred fetch).
        parent.add_condition(GraphqlCondition::Field(FieldCondition::new(
            "id",
            GraphqlOp::Eq,
            json!("5"),
        )));
        let api = parent.data_source().clone();
        let cond = api.related_in_condition::<EmptyEntity>("launch_id", &parent, "id");
        match cond {
            GraphqlCondition::Field(fc) => {
                assert_eq!(fc.field, "launch_id");
                assert_eq!(fc.op, GraphqlOp::Eq);
                assert_eq!(fc.value, json!("5"));
            }
            _ => panic!("expected sync Field, got {:?}", cond),
        }
    }

    #[test]
    fn related_in_condition_deferred_when_no_parent_eq() {
        // Parent has no eq-condition on the source column, so the
        // resolver must defer the lookup until fetch time.
        let parent = launches_table();
        let api = parent.data_source().clone();
        let cond = api.related_in_condition::<EmptyEntity>("launch_id", &parent, "mission_id");
        match cond {
            GraphqlCondition::DeferredField { field, op, .. } => {
                assert_eq!(field, "launch_id");
                assert_eq!(op, GraphqlOp::Eq);
            }
            _ => panic!("expected DeferredField, got {:?}", cond),
        }
    }

    #[tokio::test]
    async fn deferred_field_renders_through_hasura_dialect() {
        use vantage_expressions::DeferredFn;
        let cond = GraphqlCondition::DeferredField {
            field: "launch_id".into(),
            op: GraphqlOp::Eq,
            value_fn: DeferredFn::new(|| {
                Box::pin(async {
                    Ok(ExpressiveEnum::Scalar(AnyGraphqlType::new(
                        "abc-123".to_string(),
                    )))
                })
            }),
        };
        let r = cond.render(FilterDialect::Hasura).await.unwrap();
        assert_eq!(r, json!({ "launch_id": { "_eq": "abc-123" } }));
    }

    #[tokio::test]
    async fn deferred_field_renders_through_generic_dialect() {
        use vantage_expressions::DeferredFn;
        let cond = GraphqlCondition::DeferredField {
            field: "launch_id".into(),
            op: GraphqlOp::Eq,
            value_fn: DeferredFn::new(|| {
                Box::pin(async { Ok(ExpressiveEnum::Scalar(AnyGraphqlType::new(7i64))) })
            }),
        };
        let r = cond.render(FilterDialect::Generic).await.unwrap();
        assert_eq!(r, json!({ "launch_id": 7 }));
    }
}
