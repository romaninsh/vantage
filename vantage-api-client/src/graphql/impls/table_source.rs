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
use crate::graphql::types::{AnyGraphqlType, GraphqlType as _};

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
    if let Some(args) = api.root_args.clone() {
        select = select.with_root_args(args);
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

    // Conditions, orders and pagination only reach the wire when the root
    // field actually takes those arguments — otherwise they'd render a
    // query the server rejects, and the consumer applies them locally.
    if api.can_filter() {
        for cond in table.conditions() {
            select.conditions.push(cond.clone());
        }
    }

    // Orders: GraphqlCondition's `Field` variant carries the column name.
    if api.can_order() {
        for (cond, direction) in table.orders() {
            if let GraphqlCondition::Field(fc) = cond {
                let order = if matches!(direction, vantage_table::sorting::SortDirection::Ascending)
                {
                    Order::Asc
                } else {
                    Order::Desc
                };
                select.sort.push((fc.field.clone(), order));
            }
        }
    }

    if api.can_paginate()
        && let Some(pagination) = table.pagination()
    {
        select.limit = Some(pagination.limit());
        select.skip = Some(pagination.skip());
    }

    select
}

/// Walk `path` into the root field's value to reach the row array.
///
/// Each segment indexes an object, or maps over an array — so
/// `["edges", "node"]` turns `{ edges: [{ node: … }] }` into `[…]`
/// in one pass, which is every Relay-style connection.
fn descend(value: &Value, path: &[String]) -> Value {
    let Some((segment, rest)) = path.split_first() else {
        return value.clone();
    };
    let stepped = match value {
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| item.get(segment.as_str()).cloned().unwrap_or(Value::Null))
                .collect(),
        ),
        other => other.get(segment.as_str()).cloned().unwrap_or(Value::Null),
    };
    descend(&stepped, rest)
}

/// Resolve a dotted path against a row. A missing or null link yields
/// `None`, so `run.commit.hash` on a run with no commit is absent rather
/// than an error.
fn pluck<'a>(row: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cursor = row;
    for segment in path.split('.') {
        cursor = cursor.get(segment)?;
    }
    Some(cursor)
}

/// Apply conditions in memory, for a root field that takes no filter
/// argument.
///
/// Vista treats equality push-down as universal, so a consumer narrowing
/// this table never learns the server can't do it. Dropping the condition
/// at render time and returning every row would silently widen the result
/// — a relation tab would list every parent's children — so the driver
/// honours it here instead.
async fn retain_matching(rows: Vec<Value>, conditions: &[GraphqlCondition]) -> Result<Vec<Value>> {
    let mut kept = Vec::with_capacity(rows.len());
    for row in rows {
        let mut all = true;
        for condition in conditions {
            if !matches_condition(&row, condition).await? {
                all = false;
                break;
            }
        }
        if all {
            kept.push(row);
        }
    }
    Ok(kept)
}

fn matches_condition<'a>(
    row: &'a Value,
    condition: &'a GraphqlCondition,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + 'a>> {
    Box::pin(async move {
        match condition {
            GraphqlCondition::Field(fc) => Ok(compare(row, &fc.field, &fc.op, &fc.value)),
            GraphqlCondition::DeferredField {
                field,
                op,
                value_fn,
            } => {
                let resolved = value_fn.call().await?;
                let value = match resolved {
                    ExpressiveEnum::Scalar(v) => v.to_json(),
                    other => {
                        return Err(error!(
                            "Deferred filter resolved to a non-scalar",
                            got = format!("{:?}", other)
                        ));
                    }
                };
                Ok(compare(row, field, op, &value))
            }
            GraphqlCondition::And(parts) => {
                for part in parts {
                    if !matches_condition(row, part).await? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            GraphqlCondition::Or(parts) => {
                for part in parts {
                    if matches_condition(row, part).await? {
                        return Ok(true);
                    }
                }
                Ok(parts.is_empty())
            }
            GraphqlCondition::Not(inner) => Ok(!matches_condition(row, inner).await?),
            // A dynamic filter sub-object is dialect-shaped, not
            // `field op value`, so there is nothing to evaluate against a
            // row. Refusing beats quietly keeping every row.
            GraphqlCondition::Deferred(_) => Err(error!(
                "Deferred filter cannot be applied client-side; \
                 this root field accepts no filter argument"
            )),
        }
    })
}

/// Compare one row's value at `path` against `expected`. Ordering
/// comparisons work on numbers and strings; everything else falls back to
/// structural equality.
fn compare(row: &Value, path: &str, op: &GraphqlOp, expected: &Value) -> bool {
    let actual = pluck(row, path).unwrap_or(&Value::Null);
    let ordering = || match (actual.as_f64(), expected.as_f64()) {
        (Some(a), Some(b)) => a.partial_cmp(&b),
        _ => match (actual.as_str(), expected.as_str()) {
            (Some(a), Some(b)) => Some(a.cmp(b)),
            _ => None,
        },
    };
    let contains = || match expected {
        Value::Array(items) => items.iter().any(|item| item == actual),
        single => single == actual,
    };
    let text_match = |case_sensitive: bool| match (actual.as_str(), expected.as_str()) {
        (Some(a), Some(pattern)) => {
            let (a, pattern) = if case_sensitive {
                (a.to_string(), pattern.to_string())
            } else {
                (a.to_lowercase(), pattern.to_lowercase())
            };
            a.contains(pattern.trim_matches('%'))
        }
        _ => false,
    };
    match op {
        GraphqlOp::Eq => actual == expected,
        GraphqlOp::Ne => actual != expected,
        GraphqlOp::Gt => matches!(ordering(), Some(std::cmp::Ordering::Greater)),
        GraphqlOp::Gte => matches!(
            ordering(),
            Some(std::cmp::Ordering::Greater) | Some(std::cmp::Ordering::Equal)
        ),
        GraphqlOp::Lt => matches!(ordering(), Some(std::cmp::Ordering::Less)),
        GraphqlOp::Lte => matches!(
            ordering(),
            Some(std::cmp::Ordering::Less) | Some(std::cmp::Ordering::Equal)
        ),
        GraphqlOp::In => contains(),
        GraphqlOp::NotIn => !contains(),
        GraphqlOp::Like => text_match(true),
        GraphqlOp::ILike => text_match(false),
        GraphqlOp::IsNull => actual.is_null(),
        GraphqlOp::IsNotNull => !actual.is_null(),
    }
}

/// Convert a JSON row into `(Id, Record<AnyGraphqlType>)`. The id is
/// stringified from whatever JSON shape the server returned — most
/// schemas use `String` or numeric ids, both of which we coerce to
/// `String` since that's our `Id` type.
fn row_to_record(
    row: &Value,
    fields: &[String],
    id_field: Option<&str>,
) -> Result<(String, Record<AnyGraphqlType>)> {
    let obj = row
        .as_object()
        .ok_or_else(|| error!("Expected JSON object for row", got = format!("{:?}", row)))?;

    let id = match id_field {
        Some(field) => pluck(row, field)
            .map(value_to_string)
            .ok_or_else(|| error!("Row missing id field", field = field.to_string()))?,
        // No id field declared — fall back to "id" then to a stringified row index later.
        None => pluck(row, "id").map(value_to_string).unwrap_or_default(),
    };

    // Each declared path becomes a dotted key, so a nested scalar reads as
    // an ordinary flat column and a null parent still leaves the column
    // present (as null) rather than missing.
    let mut out: IndexMap<String, AnyGraphqlType> = IndexMap::new();
    if fields.is_empty() {
        for (k, v) in obj {
            out.insert(k.clone(), AnyGraphqlType::untyped(v.clone()));
        }
    } else {
        for path in fields {
            let value = pluck(row, path).cloned().unwrap_or(Value::Null);
            out.insert(path.clone(), AnyGraphqlType::untyped(value));
        }
    }

    Ok((id, Record::from_indexmap(out)))
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
        let rows = descend(rows, table.data_source().response_path());

        let arr = match rows {
            Value::Array(a) => a,
            Value::Null => Vec::new(),
            other => {
                return Err(error!(
                    "Unexpected response shape — expected array under root field",
                    root = root.to_string(),
                    got = format!("{:?}", other)
                ));
            }
        };

        // Conditions the query couldn't carry are applied here, so a
        // narrowed table returns narrowed rows either way.
        let arr = if table.data_source().can_filter() {
            arr
        } else {
            retain_matching(arr, &table.conditions().cloned().collect::<Vec<_>>()).await?
        };

        let id_name = table.id_field().map(|c| c.name().to_string());
        let mut out = IndexMap::with_capacity(arr.len());
        for (idx, row) in arr.iter().enumerate() {
            let (mut id, rec) = row_to_record(row, &select.fields, id_name.as_deref())?;
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
        let rows = descend(rows, table.data_source().response_path());

        let arr = match rows {
            Value::Array(a) => a,
            Value::Null => return Ok(None),
            // Some `byId`-style root fields return a single object instead of an array.
            obj @ Value::Object(_) => vec![obj],
            other => {
                return Err(error!(
                    "Unexpected response shape for get",
                    got = format!("{:?}", other)
                ));
            }
        };
        match arr.into_iter().next() {
            Some(row) => {
                let (_id, rec) = row_to_record(&row, &select.fields, Some(&id_name))?;
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
        let (id, rec) = row_to_record(&row, &[], Some("id")).unwrap();
        assert_eq!(id, "5");
        assert_eq!(rec.iter().count(), 3);
    }

    #[test]
    fn row_to_record_stringifies_numeric_id() {
        let row = json!({ "id": 42, "name": "x" });
        let (id, _rec) = row_to_record(&row, &[], Some("id")).unwrap();
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

#[cfg(test)]
mod shape_tests {
    use super::*;
    use serde_json::json;
    use vantage_types::EmptyEntity;

    use crate::graphql::types::GraphqlType as _;

    fn paths(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn descend_unwraps_a_relay_connection() {
        let payload = json!({
            "total": 2,
            "edges": [{ "node": { "run": { "id": "a" } } }, { "node": { "run": { "id": "b" } } }]
        });
        let rows = descend(&payload, &paths(&["edges", "node"]));
        assert_eq!(
            rows,
            json!([{ "run": { "id": "a" } }, { "run": { "id": "b" } }])
        );
    }

    #[test]
    fn descend_with_an_empty_path_is_the_identity() {
        let payload = json!([{ "id": "a" }]);
        assert_eq!(descend(&payload, &[]), payload);
    }

    #[test]
    fn declared_paths_become_dotted_columns() {
        let row = json!({ "run": { "id": "a", "commit": { "hash": "deadbeef" } }, "stack": { "id": "api-uat" } });
        let fields = paths(&["run.id", "run.commit.hash", "stack.id"]);
        let (id, rec) = row_to_record(&row, &fields, Some("run.id")).unwrap();
        assert_eq!(id, "a");
        assert_eq!(
            rec.get("run.commit.hash").map(|v| v.to_json()),
            Some(json!("deadbeef"))
        );
        assert_eq!(
            rec.get("stack.id").map(|v| v.to_json()),
            Some(json!("api-uat"))
        );
    }

    #[test]
    fn a_null_parent_leaves_the_column_present_and_null() {
        let row = json!({ "run": { "id": "a", "commit": null } });
        let fields = paths(&["run.id", "run.commit.hash"]);
        let (_id, rec) = row_to_record(&row, &fields, Some("run.id")).unwrap();
        assert_eq!(
            rec.get("run.commit.hash").map(|v| v.to_json()),
            Some(json!(null))
        );
    }

    #[test]
    fn conditions_stay_local_when_the_root_field_takes_no_filter() {
        let api = GraphqlApi::builder("https://api.test/graphql")
            .supports(crate::graphql::api::Supports {
                filter: Some(false),
                ..Default::default()
            })
            .build();
        let mut table = Table::<GraphqlApi, EmptyEntity>::new("stacks", api).with_id_column("id");
        table.add_condition(GraphqlCondition::Field(
            crate::graphql::condition::FieldCondition::new("space", GraphqlOp::Eq, json!("root")),
        ));
        assert!(select_from_table(&table).conditions.is_empty());
    }
}

#[cfg(test)]
mod local_filter_tests {
    use super::*;
    use crate::graphql::condition::FieldCondition;
    use serde_json::json;

    fn rows() -> Vec<Value> {
        vec![
            json!({ "stack": { "id": "api-uat" }, "run": { "id": "1", "delta": { "addCount": 4 } } }),
            json!({ "stack": { "id": "api-dev" }, "run": { "id": "2", "delta": { "addCount": 0 } } }),
        ]
    }

    /// The narrowing a binder tab applies has to survive even though the
    /// root field takes no filter argument — otherwise the tab would list
    /// every stack's runs.
    #[tokio::test]
    async fn equality_on_a_dotted_path_narrows_locally() {
        let cond = GraphqlCondition::Field(FieldCondition::new(
            "stack.id",
            GraphqlOp::Eq,
            json!("api-uat"),
        ));
        let kept = retain_matching(rows(), &[cond]).await.unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(pluck(&kept[0], "run.id"), Some(&json!("1")));
    }

    #[tokio::test]
    async fn ordering_comparisons_work_on_numbers() {
        let cond = GraphqlCondition::Field(FieldCondition::new(
            "run.delta.addCount",
            GraphqlOp::Gt,
            json!(1),
        ));
        let kept = retain_matching(rows(), &[cond]).await.unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(pluck(&kept[0], "stack.id"), Some(&json!("api-uat")));
    }

    /// A filter that can't be evaluated row-wise must fail rather than
    /// quietly returning everything.
    #[tokio::test]
    async fn an_opaque_deferred_filter_is_refused() {
        let cond = GraphqlCondition::Deferred(DeferredFn::new(|| {
            Box::pin(async { Ok(ExpressiveEnum::Scalar(AnyGraphqlType::new(1i64))) })
        }));
        assert!(retain_matching(rows(), &[cond]).await.is_err());
    }
}
