use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::error;
use vantage_dataset::traits::{ReadableValueSet, Result};
use vantage_expressions::Expression;
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_expressions::traits::datasource::DataSource;
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::expressive::DeferredFn;

use crate::RestApi;

/// Extract the id field name from a table's column flags.
fn id_field_name<E: Entity<CborValue>>(table: &Table<RestApi, E>) -> Option<String> {
    table.id_field().map(|col| col.name().to_string())
}

impl ExprDataSource<CborValue> for RestApi {
    async fn execute(&self, expr: &Expression<CborValue>) -> vantage_core::Result<CborValue> {
        if expr.parameters.is_empty() {
            Ok(CborValue::Text(expr.template.clone()))
        } else {
            Ok(CborValue::Null)
        }
    }

    fn defer(&self, expr: Expression<CborValue>) -> DeferredFn<CborValue> {
        let api = self.clone();
        DeferredFn::new(move || {
            let api = api.clone();
            let expr = expr.clone();
            Box::pin(async move {
                let result = api.execute(&expr).await?;
                Ok(ExpressiveEnum::Scalar(result))
            })
        })
    }
}

impl DataSource for RestApi {}

#[async_trait]
impl TableSource for RestApi {
    type Column<Type>
        = Column<Type>
    where
        Type: ColumnType;
    type AnyType = CborValue;
    type Value = CborValue;
    type Id = String;
    type Condition = vantage_expressions::Expression<Self::Value>;

    /// Build a stringy `field == value` eq-condition. Stringy callers
    /// (the model-driven CLI) only have text on hand; values arrive as
    /// `String` and become CBOR text scalars here. The peeling code
    /// in `condition_to_query_param` renders all scalar variants the
    /// same way so the URL still reads correctly.
    fn eq_condition(field: &str, value: &str) -> Result<Self::Condition> {
        Ok(crate::eq_condition(field, value.to_string()))
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
    ) -> Expression<Self::Value>
    where
        E: Entity<Self::Value>,
    {
        Expression::new(format!("SEARCH '{}'", search_value), vec![])
    }

    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        self.fetch_records(
            table.table_name(),
            id_field_name(table).as_deref(),
            table.pagination(),
            table.conditions(),
        )
        .await
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
        let records = self
            .fetch_records(
                table.table_name(),
                id_field_name(table).as_deref(),
                table.pagination(),
                table.conditions(),
            )
            .await?;
        Ok(records.get(id).cloned())
    }

    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let records = self
            .fetch_records(
                table.table_name(),
                id_field_name(table).as_deref(),
                table.pagination(),
                table.conditions(),
            )
            .await?;
        Ok(records.into_iter().next())
    }

    async fn get_table_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let records = self
            .fetch_records(
                table.table_name(),
                id_field_name(table).as_deref(),
                table.pagination(),
                table.conditions(),
            )
            .await?;
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
        Err(error!("Sum not implemented for API backend"))
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
        Err(error!("Max not implemented for API backend"))
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
        Err(error!("Min not implemented for API backend"))
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
        Err(error!("REST API is a read-only data source"))
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
        Err(error!("REST API is a read-only data source"))
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
        Err(error!("REST API is a read-only data source"))
    }

    async fn delete_table_value<E>(&self, _table: &Table<Self, E>, _id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("REST API is a read-only data source"))
    }

    async fn delete_table_all_values<E>(&self, _table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("REST API is a read-only data source"))
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
        Err(error!("REST API is a read-only data source"))
    }

    /// Build a child condition for `with_many` / `with_one` traversal.
    ///
    /// REST APIs can't run subqueries, so this resolves on two paths:
    ///
    /// * **Sync peek** — if the parent already carries an eq-condition
    ///   on `source_column`, we re-key its value onto `target_field`
    ///   and we're done. Covers the common `with_many` case where
    ///   `source_column` is the parent's id field (narrowed via
    ///   `id=N` or `[N]`).
    /// * **Deferred read** — otherwise (the `with_one` case, where
    ///   `source_column` is a foreign-key field that lives in the
    ///   parent's record, not its conditions), we wrap the resolution
    ///   in a `DeferredFn` that fetches the parent at request time and
    ///   pulls the field out of the row. `fetch_records` resolves
    ///   deferreds before peeling conditions into query params.
    fn related_in_condition<SourceE: Entity<Self::Value> + 'static>(
        &self,
        target_field: &str,
        source_table: &Table<Self, SourceE>,
        source_column: &str,
    ) -> Self::Condition
    where
        Self: Sized,
    {
        for cond in source_table.conditions() {
            if let Some((field, value)) = crate::condition_to_query_param(cond)
                && field == source_column
            {
                let cbor_value: CborValue = if let Ok(i) = value.parse::<i64>() {
                    CborValue::Integer(i.into())
                } else if let Ok(f) = value.parse::<f64>() {
                    CborValue::Float(f)
                } else {
                    CborValue::Text(value)
                };
                return crate::eq_condition(target_field, cbor_value);
            }
        }

        // Deferred fallback. Clone the parent table into the closure
        // so it stays valid past this stack frame; at fetch time we
        // list its values, take the first, and pull `source_column`.
        let parent = source_table.clone();
        let column = source_column.to_string();
        let parent_name = source_table.table_name().to_string();
        let deferred = DeferredFn::new(move || {
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

        Expression::new(
            "{} = {}",
            vec![
                ExpressiveEnum::Nested(Expression::new(target_field.to_string(), vec![])),
                ExpressiveEnum::Nested(Expression::new(
                    "{}",
                    vec![ExpressiveEnum::Deferred(deferred)],
                )),
            ],
        )
    }

    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> AssociatedExpression<'a, Self, Self::Value, Vec<Type>>
    where
        E: Entity<Self::Value> + 'static,
        Self: Sized,
    {
        unimplemented!("column_table_values_expr not yet supported for REST API")
    }
}
