//! `TableSource` for `GraphqlApi`.
//!
//! Bridges `Table<GraphqlApi, E>` to `GraphqlSelect`-driven HTTP requests.
//! Each call builds a select from the table's current state (columns →
//! selection set, conditions → filter arg, orders → `order_by`, pagination
//! → `$limit/$offset` variables), runs it, and reshapes the response.
//!
//! Reads and count only. Writes (insert/update/delete) and SQL-style
//! aggregates (sum/max/min) return `Unsupported` errors: mutation shapes
//! differ too much between schemas (Hasura mutations, hand-rolled
//! `createUser`, Postgraphile `userCreate`) to have one rendering.

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
use crate::graphql::impls::local_filter::{pluck, retain_matching};
use crate::graphql::operation::GraphqlOperation;
use crate::graphql::select::GraphqlSelect;
use crate::graphql::types::AnyGraphqlType;
use crate::row_keys::insert_row;

/// The error every write and server-side aggregate returns.
fn unsupported(method: &'static str) -> vantage_core::VantageError {
    error!(
        "GraphQL data source supports reads and count only",
        method = method
    )
    .mark_unsupported()
}

/// The select a table's current state describes: root field from the table
/// name, the id field then the columns as fields, and its conditions, orders
/// and pagination where the API takes them. Dialect, filter argument, root
/// arguments and response path come from the API.
pub(crate) fn select_from_table<E: Entity<AnyGraphqlType>>(
    table: &Table<GraphqlApi, E>,
) -> GraphqlSelect {
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
    if !api.response_path().is_empty() {
        select = select.with_response_path(api.response_path().to_vec());
    }

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
    for (name, field) in &api.field_aliases {
        select = select.with_alias(name, field);
    }

    // Conditions, orders and pagination only reach the wire when the root
    // field actually takes those arguments — otherwise they'd render a
    // query the server rejects, and the consumer applies them locally.
    if api.can_filter() {
        for cond in table.conditions() {
            select.conditions.push(cond.clone());
        }
    }

    // An order is stored as a `Field` condition, whose field is the column.
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

    // A page sized by the server and then filtered locally would come back
    // short, so `limit`/`offset` go out only with filter push-down on.
    if api.can_page()
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
pub(crate) fn descend(value: &Value, path: &[String]) -> Value {
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

/// Convert a JSON row into `(Id, Record<AnyGraphqlType>)`. The id is
/// stringified from whatever JSON shape the server returned.
pub(crate) fn row_to_record(
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
        // No id field declared: fall back to "id", and with none of that
        // either the caller keys the row by its position.
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

/// Run `select` and return the JSON rows under the root field, with any
/// conditions the query couldn't carry applied locally — so a narrowed
/// table returns narrowed rows either way.
pub(crate) async fn list_rows<E: Entity<AnyGraphqlType>>(
    table: &Table<GraphqlApi, E>,
    select: &GraphqlSelect,
) -> Result<Vec<Value>> {
    let api = table.data_source();
    let rendered = select.render().await?;
    let data = api
        .post_graphql(&rendered.query, &rendered.variables)
        .await?;

    let arr = rows_under_root(api, table.table_name(), &data, false)?;

    if api.can_filter() {
        Ok(arr)
    } else {
        retain_matching(arr, &table.conditions().cloned().collect::<Vec<_>>()).await
    }
}

/// The rows under `root` in a response's `data`, reached by walking the
/// API's response path. A null reads as no rows. With `single_object`, an
/// object reads as one row, for `byId`-style root fields that return the
/// row itself rather than an array.
pub(crate) fn rows_under_root(
    api: &GraphqlApi,
    root: &str,
    data: &Value,
    single_object: bool,
) -> Result<Vec<Value>> {
    let rows = data
        .get(root)
        .ok_or_else(|| error!("GraphQL response missing root field", field = root))?;
    match descend(rows, api.response_path()) {
        Value::Array(a) => Ok(a),
        Value::Null => Ok(Vec::new()),
        obj @ Value::Object(_) if single_object => Ok(vec![obj]),
        other => Err(error!(
            "Unexpected response shape — expected array under root field",
            root = root,
            got = format!("{:?}", other)
        )),
    }
}

pub(crate) fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// A GraphQL condition as text: `field op value`, parts joined by `AND`/`OR`.
fn describe_graphql(condition: &GraphqlCondition) -> String {
    let symbol = |op: &GraphqlOp| match op {
        GraphqlOp::Eq => "=",
        GraphqlOp::Ne => "!=",
        GraphqlOp::Gt => ">",
        GraphqlOp::Gte => ">=",
        GraphqlOp::Lt => "<",
        GraphqlOp::Lte => "<=",
        GraphqlOp::In => "IN",
        GraphqlOp::NotIn => "NOT IN",
        GraphqlOp::Like => "LIKE",
        GraphqlOp::ILike => "ILIKE",
        GraphqlOp::IsNull => "IS NULL",
        GraphqlOp::IsNotNull => "IS NOT NULL",
    };
    let join = |parts: &[GraphqlCondition], sep: &str| {
        parts
            .iter()
            .map(describe_graphql)
            .collect::<Vec<_>>()
            .join(sep)
    };
    match condition {
        GraphqlCondition::Field(fc) => match fc.op {
            GraphqlOp::IsNull | GraphqlOp::IsNotNull => format!("{} {}", fc.field, symbol(&fc.op)),
            _ => format!("{} {} {}", fc.field, symbol(&fc.op), fc.value),
        },
        GraphqlCondition::DeferredField { field, op, .. } => {
            format!("{field} {} <deferred>", symbol(op))
        }
        GraphqlCondition::And(parts) => join(parts, " AND "),
        GraphqlCondition::Or(parts) => format!("({})", join(parts, " OR ")),
        GraphqlCondition::Not(inner) => format!("NOT ({})", describe_graphql(inner)),
        GraphqlCondition::Deferred(_) => "<deferred>".to_string(),
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

    fn in_values_condition(
        &self,
        field: &str,
        values: Vec<Self::Value>,
    ) -> Result<Self::Condition> {
        Ok(Column::<AnyGraphqlType>::new(field).in_(values))
    }

    fn describe_condition(&self, condition: &Self::Condition) -> String {
        describe_graphql(condition)
    }

    fn condition_column(&self, condition: &Self::Condition) -> Option<String> {
        let narrows = |op: &GraphqlOp| {
            !matches!(
                op,
                GraphqlOp::Ne | GraphqlOp::NotIn | GraphqlOp::IsNull | GraphqlOp::IsNotNull
            )
        };
        match condition {
            GraphqlCondition::Field(fc) if narrows(&fc.op) => Some(fc.field.clone()),
            GraphqlCondition::DeferredField { field, op, .. } if narrows(op) => Some(field.clone()),
            _ => None,
        }
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

    /// An OR of `_ilike '%value%'` over the searchable columns: those
    /// flagged `searchable`, or else the string-typed, non-id, undotted
    /// ones. Hasura speaks `_ilike` natively; Generic rejects it at render time,
    /// which is the right failure mode (search doesn't translate to
    /// flat-arg schemas).
    fn search_table_condition<E>(
        &self,
        table: &Table<Self, E>,
        search_value: &str,
    ) -> Self::Condition
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        crate::graphql::impls::search::search_condition(table, search_value)
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
        let arr = list_rows(table, &select).await?;

        let id_name = table.id_field().map(|c| c.name().to_string());
        let offset = select.skip.unwrap_or(0).max(0) as usize;
        let mut out = IndexMap::with_capacity(arr.len());
        for (idx, row) in arr.iter().enumerate() {
            let (id, rec) = row_to_record(row, &select.fields, id_name.as_deref())?;
            insert_row(&mut out, Some(id), offset + idx, rec)?;
        }
        self.report_rows(out.len());
        Ok(out)
    }

    /// Single-row fetch by id: the table's own narrowing plus `id = <id>`.
    /// Without filter push-down the rows are listed and the id picked out
    /// locally.
    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Option<Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        crate::graphql::impls::get_by_id::get_by_id(self, table, id).await
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

    /// Hasura counts server-side through its `_aggregate` field; every
    /// other schema lists the rows and counts them.
    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        if table.data_source().can_count_server_side() {
            return crate::graphql::impls::count::aggregate_count(table).await;
        }
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
        Err(unsupported("get_table_sum"))
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
        Err(unsupported("get_table_max"))
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
        Err(unsupported("get_table_min"))
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
        Err(unsupported("insert_table_value"))
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
        Err(unsupported("replace_table_value"))
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
        Err(unsupported("patch_table_value"))
    }

    async fn delete_table_value<E>(&self, _table: &Table<Self, E>, _id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(unsupported("delete_table_value"))
    }

    async fn delete_table_all_values<E>(&self, _table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(unsupported("delete_table_all_values"))
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
        Err(unsupported("insert_table_return_id_value"))
    }

    /// Build a child condition for `with_many` / `with_one` traversal.
    ///
    /// Two paths, as in the REST adapter:
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
    /// The deferred path costs two round trips: one for the parent, one for
    /// the child. A nested selection could do it in one, but the `Table`
    /// trait surface has no way to express it.
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

        GraphqlCondition::deferred_field(target_field, GraphqlOp::Eq, value_fn)
    }

    /// Defer to a query that selects just `column`. Used by the
    /// `Table::with_one` / `Table::with_many` plumbing when it wants a
    /// list of values from another table (e.g. all FK ids).
    ///
    /// At fetch time the table's rows are listed through
    /// `list_table_values` and `column` is read from each. A Generic schema
    /// has no sub-select to push this into, so every dialect lists.
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
