use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use indexmap::IndexMap;
use serde_json::Value;
use tokio_stream::StreamExt;
use vantage_core::error;
use vantage_dataset::traits::Result;
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_expressions::traits::datasource::{DataSource, ExprDataSource};
use vantage_expressions::traits::expressive::{DeferredFn, ExpressiveEnum};
use vantage_expressions::Expression;
use vantage_table::column::core::{Column, ColumnType};
use vantage_table::table::Table;
use vantage_table::traits::table_like::TableLike;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{Entity, Record};

use crate::{AwwPool, PaginatedStream};

/// Vantage `TableSource` backed by `AwwPool` with automatic pagination.
///
/// Wraps `Arc<AwwPool>` so it's cheaply cloneable (required by `TableSource`).
/// - `list_table_values` auto-paginates, collecting all pages.
/// - `stream_table_values` returns a `PaginatedStream` for incremental processing.
#[derive(Clone)]
pub struct PoolApi {
    pool: Arc<AwwPool>,
    id_field: Option<String>,
}

impl std::fmt::Debug for PoolApi {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PoolApi")
            .field("id_field", &self.id_field)
            .finish()
    }
}

impl PoolApi {
    pub fn new(pool: Arc<AwwPool>) -> Self {
        Self {
            pool,
            id_field: None,
        }
    }

    /// Get a reference to the underlying pool.
    pub fn pool(&self) -> &Arc<AwwPool> {
        &self.pool
    }

    fn id_field_for<E: Entity<Value>>(&self, table: &Table<Self, E>) -> Option<String> {
        table
            .id_field()
            .map(|col| col.name().to_string())
            .or_else(|| self.id_field.clone())
    }
}

impl DataSource for PoolApi {}

impl ExprDataSource<Value> for PoolApi {
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

#[async_trait]
impl TableSource for PoolApi {
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

    /// Fetch all records by streaming through all pages.
    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let id_field = self.id_field_for(table);
        let endpoint = format!("/{}", table.table_name());
        let mut stream = PaginatedStream::get(self.pool.clone(), endpoint).prefetch(3);

        let mut records = IndexMap::new();
        while let Some(item) = stream.next().await {
            let item = item.map_err(|e| error!("Stream error", detail = e))?;
            let obj = item
                .as_object()
                .ok_or_else(|| error!("API data item is not an object"))?;

            let id = id_field
                .as_deref()
                .and_then(|field| obj.get(field))
                .and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .unwrap_or_else(|| records.len().to_string());

            let record: Record<Value> = obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
            records.insert(id, record);
        }

        Ok(records)
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
        // Fetch all and find by id — could be optimized with a direct endpoint later
        let records = self.list_table_values(table).await?;
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
        let records = self.list_table_values(table).await?;
        Ok(records.into_iter().next())
    }

    async fn get_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        let records = self.list_table_values(table).await?;
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
        Err(error!("Sum not implemented for API pool backend"))
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
        Err(error!("Max not implemented for API pool backend"))
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
        Err(error!("Min not implemented for API pool backend"))
    }

    /// Stream records page by page using PaginatedStream with prefetch.
    fn stream_table_values<'a, E>(
        &'a self,
        table: &Table<Self, E>,
    ) -> Pin<Box<dyn Stream<Item = Result<(Self::Id, Record<Self::Value>)>> + Send + 'a>>
    where
        E: Entity<Self::Value> + 'a,
        Self: Sized,
    {
        let id_field = self.id_field_for(table);
        let endpoint = format!("/{}", table.table_name());
        let pool = self.pool.clone();

        Box::pin(async_stream::stream! {
            let mut stream = PaginatedStream::get(pool, endpoint).prefetch(3);
            let mut row_idx = 0usize;

            while let Some(item) = stream.next().await {
                let item = match item {
                    Ok(v) => v,
                    Err(e) => {
                        yield Err(error!("Stream error", detail = e));
                        return;
                    }
                };

                let result = match item.as_object() {
                    Some(obj) => {
                        let id = id_field
                            .as_deref()
                            .and_then(|field| obj.get(field))
                            .and_then(|v| match v {
                                Value::String(s) => Some(s.clone()),
                                Value::Number(n) => Some(n.to_string()),
                                _ => None,
                            })
                            .unwrap_or_else(|| row_idx.to_string());

                        let record: Record<Value> =
                            obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                        row_idx += 1;
                        Ok((id, record))
                    }
                    None => Err(error!("API data item is not an object")),
                };

                yield result;
            }
        })
    }

    // Write operations — not supported
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
        Err(error!("REST API pool is a read-only data source"))
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
        Err(error!("REST API pool is a read-only data source"))
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
        Err(error!("REST API pool is a read-only data source"))
    }

    async fn delete_table_value<E>(&self, _table: &Table<Self, E>, _id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("REST API pool is a read-only data source"))
    }

    async fn delete_table_all_values<E>(&self, _table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized,
    {
        Err(error!("REST API pool is a read-only data source"))
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
        Err(error!("REST API pool is a read-only data source"))
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
        unimplemented!("column_table_values_expr not yet supported for API pool")
    }
}
