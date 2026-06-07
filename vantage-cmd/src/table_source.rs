//! `TableSource` impl for `Cmd`.
//!
//! The one interesting method is `list_table_values`: it resolves any
//! deferred (relation) conditions, hands the read context to the Rhai
//! script on a blocking thread (which builds the argv, runs the locked
//! command, and parses the output), then keys the resulting rows by their
//! id field and applies a client-side `Eq` filter as a safety net.
//!
//! Read-only: writes and aggregations error, exactly like `vantage-aws`.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use serde_json::{Value as JsonValue, json};

use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_expressions::{
    Expression, Expressive, expr_any,
    traits::associated_expressions::AssociatedExpression,
    traits::datasource::ExprDataSource,
    traits::expressive::{DeferredFn, ExpressiveEnum},
};
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::cmd::Cmd;
use crate::condition::CmdCondition;
use crate::rhai_engine::QueryContext;
use crate::types::{cbor_to_string, json_to_cbor};

/// Convert script-produced JSON rows into id-keyed records. The id comes
/// from the `id_field` value on each row, falling back to the row index.
fn rows_to_records(
    rows: Vec<JsonValue>,
    id_field: Option<&str>,
) -> IndexMap<String, Record<CborValue>> {
    let mut records: IndexMap<String, Record<CborValue>> = IndexMap::new();
    for (idx, row) in rows.into_iter().enumerate() {
        let mut record = Record::new();
        match row {
            JsonValue::Object(map) => {
                for (k, v) in map {
                    record.insert(k, json_to_cbor(&v));
                }
            }
            other => {
                record.insert("value".to_string(), json_to_cbor(&other));
            }
        }
        let id = id_field
            .and_then(|f| record.get(f))
            .map(cbor_to_string)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| idx.to_string());
        records.insert(id, record);
    }
    records
}

impl Cmd {
    /// Detail-script hydration for one id, with the existing list-pass `row`
    /// injected into the script scope as `row`. Falls back to the normal
    /// list-and-pick path when the table has no detail script.
    pub async fn get_table_value_with_row<E>(
        &self,
        table: &Table<Self, E>,
        id: &String,
        row: &Record<CborValue>,
    ) -> Result<Option<Record<CborValue>>>
    where
        E: Entity<CborValue>,
        Self: Sized,
    {
        let table_name = table.table_name().to_string();
        if !self.has_detail_script(&table_name) {
            let mut all = self.list_table_values(table).await?;
            return Ok(all.shift_remove(id));
        }

        let id_field = table.id_field().map(|c| c.name().to_string());
        let columns: Vec<String> = table.columns().keys().cloned().collect();
        let row_map = ciborium::value::Value::Map(
            row.iter()
                .map(|(k, v)| (ciborium::value::Value::Text(k.clone()), v.clone()))
                .collect(),
        );
        let ctx = QueryContext {
            conditions: Vec::new(),
            columns,
            limit: None,
            offset: None,
            id_column: id_field.clone(),
            id: Some(id.clone()),
            row: row_map,
        };
        let cmd = self.clone();
        let name = table_name.clone();
        let rows: Vec<JsonValue> = tokio::task::spawn_blocking(move || {
            let compiled = cmd
                .compiled_detail_script(&name)?
                .ok_or_else(|| error!("detail script vanished"))?;
            compiled.eval(ctx)
        })
        .await
        .map_err(|e| error!("command task failed to join", detail = e.to_string()))??;

        let mut records = rows_to_records(rows, id_field.as_deref());
        // Prefer the row matching the requested id; else the first row.
        Ok(records
            .shift_remove(id)
            .or_else(|| records.into_iter().next().map(|(_, r)| r)))
    }
}

#[async_trait]
impl TableSource for Cmd {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = CborValue;
    type Value = CborValue;
    type Id = String;
    type Condition = CmdCondition;
    type Source = String;

    fn eq_condition(field: &str, value: &str) -> Result<Self::Condition> {
        Ok(CmdCondition::eq(field.to_string(), value.to_string()))
    }

    fn eq_value_condition(&self, field: &str, value: Self::Value) -> Result<Self::Condition> {
        Ok(CmdCondition::eq(field.to_string(), value))
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

    fn search_table_condition<E>(
        &self,
        _table: &Table<Self, E>,
        search_value: &str,
    ) -> Self::Condition
    where
        E: Entity<Self::Value>,
    {
        // The script decides how to use a `__search__` condition (e.g.
        // CloudWatch's `--filter-pattern`); there's no generic field set
        // at this layer.
        CmdCondition::eq("__search__", json!(search_value).to_string())
    }

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let table_name = table.table_name().to_string();

        let id_field = table.id_field().map(|c| c.name().to_string());

        // Resolve Deferred (relation) conditions to concrete Eq/In before
        // anything reaches the script — the command never sees a subquery.
        let mut conditions: Vec<CmdCondition> = Vec::new();
        for cond in table.conditions().cloned() {
            match cond {
                CmdCondition::Deferred { field, source } => {
                    let resolved = ExprDataSource::execute(self, &source).await?;
                    match resolved {
                        CborValue::Array(values) => {
                            conditions.push(CmdCondition::In { field, values })
                        }
                        other => conditions.push(CmdCondition::Eq {
                            field,
                            value: other,
                        }),
                    }
                }
                other => conditions.push(other),
            }
        }

        let columns: Vec<String> = table.columns().keys().cloned().collect();
        let (limit, offset) = match table.pagination() {
            Some(p) => (Some(p.limit()), Some(p.skip())),
            None => (None, None),
        };

        let ctx = QueryContext {
            conditions: conditions.clone(),
            columns,
            limit,
            offset,
            id_column: id_field.clone(),
            id: None,
            row: ciborium::value::Value::Map(vec![]),
        };

        let cmd = self.clone();
        let rows: Vec<JsonValue> = tokio::task::spawn_blocking(move || {
            let compiled = cmd.compiled_list_script(&table_name)?;
            compiled.eval(ctx)
        })
        .await
        .map_err(|e| error!("command task failed to join", detail = e.to_string()))??;

        let mut records = rows_to_records(rows, id_field.as_deref());

        // Client-side safety net: re-apply `Eq` conditions naming real
        // record fields. Fields the script consumed as request flags won't
        // appear on the rows and are left alone.
        records.retain(|_id, record| {
            conditions.iter().all(|c| match c {
                CmdCondition::Eq { field, value } => match record.get(field) {
                    Some(rec_val) => rec_val == value,
                    None => true,
                },
                _ => true,
            })
        });

        Ok(records)
    }

    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Option<Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        // Detail hydration with no caller-supplied row (id-only path).
        let empty: Record<CborValue> = Record::new();
        self.get_table_value_with_row(table, id, &empty).await
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let all = self.list_table_values(table).await?;
        Ok(all.into_iter().next())
    }

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let all = self.list_table_values(table).await?;
        Ok(all.len() as i64)
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
        Err(error!("Aggregations not supported by vantage-cmd"))
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
        Err(error!("Aggregations not supported by vantage-cmd"))
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
        Err(error!("Aggregations not supported by vantage-cmd"))
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
        Err(error!("vantage-cmd is read-only"))
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
        Err(error!("vantage-cmd is read-only"))
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
        Err(error!("vantage-cmd is read-only"))
    }

    async fn delete_table_value<E>(&self, _table: &Table<Self, E>, _id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("vantage-cmd is read-only"))
    }

    async fn delete_table_all_values<E>(&self, _table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("vantage-cmd is read-only"))
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
        Err(error!("vantage-cmd is read-only"))
    }

    fn related_in_condition<SourceE: Entity<Self::Value> + 'static>(
        &self,
        target_field: &str,
        source_table: &Table<Self, SourceE>,
        source_column: &str,
    ) -> Self::Condition
    where
        Self: Sized,
    {
        // "target_field IN (subquery)" as a Deferred condition, resolved
        // at read time in `list_table_values`. Same shape as vantage-aws.
        let src_col = self.create_column::<Self::AnyType>(source_column);
        let values_expr = self.column_table_values_expr(source_table, &src_col);
        CmdCondition::Deferred {
            field: target_field.to_string(),
            source: values_expr.expr(),
        }
    }

    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        table: &Table<Self, E>,
        column: &Self::Column<Type>,
    ) -> AssociatedExpression<'a, Self, Self::Value, Vec<Type>>
    where
        E: Entity<Self::Value> + 'static,
        Self: Sized,
    {
        let table_clone = table.clone();
        let col = column.name().to_string();
        let cmd = self.clone();

        let inner = expr_any!("{}", {
            DeferredFn::new(move || {
                let cmd = cmd.clone();
                let table = table_clone.clone();
                let col = col.clone();
                Box::pin(async move {
                    let records = cmd.list_table_values(&table).await?;
                    let values: Vec<CborValue> = records
                        .values()
                        .filter_map(|r| r.get(&col).cloned())
                        .collect();
                    Ok(ExpressiveEnum::Scalar(CborValue::Array(values)))
                })
            })
        });

        let expr = expr_any!("{}", { self.defer(inner) });
        AssociatedExpression::new(expr, self)
    }
}
