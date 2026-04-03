use async_trait::async_trait;
use indexmap::IndexMap;
use serde_json::Value;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_expressions::Expression;
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_expressions::traits::datasource::DataSource;
use vantage_expressions::traits::expressive::ExpressiveEnum;
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::table_like::TableLike;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::expressive::DeferredFn;

use crate::RestApi;

/// Extract the id field name from a table's column flags.
fn id_field_name<E: Entity<Value>>(table: &Table<RestApi, E>) -> Option<String> {
    table.id_field().map(|col| col.name().to_string())
}

impl ExprDataSource<Value> for RestApi {
    async fn execute(&self, expr: &Expression<Value>) -> vantage_core::Result<Value> {
        if expr.parameters.is_empty() {
            Ok(Value::String(expr.template.clone()))
        } else {
            Ok(Value::Null)
        }
    }

    fn defer(&self, expr: Expression<Value>) -> DeferredFn<Value> {
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
    type AnyType = Value;
    type Value = Value;
    type Id = String;

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

    fn search_table_expr(
        &self,
        _table: &impl TableLike,
        search_value: &str,
    ) -> Expression<Self::Value> {
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
        self.fetch_records(table.table_name(), id_field_name(table).as_deref())
            .await
    }

    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let records = self
            .fetch_records(table.table_name(), id_field_name(table).as_deref())
            .await?;
        records
            .get(id)
            .cloned()
            .ok_or_else(|| error!("Record not found", id = id))
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
            .fetch_records(table.table_name(), id_field_name(table).as_deref())
            .await?;
        Ok(records.into_iter().next())
    }

    async fn get_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let records = self
            .fetch_records(table.table_name(), id_field_name(table).as_deref())
            .await?;
        Ok(records.len() as i64)
    }

    async fn get_sum<E>(
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

    async fn get_max<E>(
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

    async fn get_min<E>(
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

    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        _table: &Table<Self, E>,
        _column: &Self::Column<Type>,
    ) -> AssociatedExpression<'a, Self, Self::Value, Vec<Type>>
    where
        E: Entity<Self::Value> + 'static,
        Self: Sized,
    {
        // TODO: implement when conditions/expressions are added
        unimplemented!("column_table_values_expr not yet supported for REST API")
    }
}
