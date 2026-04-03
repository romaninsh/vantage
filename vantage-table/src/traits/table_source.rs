use std::hash::Hash;
use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use indexmap::IndexMap;
use vantage_dataset::traits::Result;
use vantage_expressions::{
    Expression,
    traits::associated_expressions::AssociatedExpression,
    traits::datasource::{DataSource, ExprDataSource},
    traits::expressive::ExpressiveEnum,
};
use vantage_types::{Entity, Record};

use crate::{
    column::core::ColumnType,
    table::Table,
    traits::{column_like::ColumnLike, table_like::TableLike},
};

/// Trait for table data sources that defines column type separate from execution
/// TableSource represents a data source that can create and manage tables
#[async_trait]
pub trait TableSource: DataSource + Clone + 'static {
    type Column<Type>: ColumnLike<Type> + Clone
    where
        Type: ColumnType;
    type AnyType: ColumnType;
    type Value: Clone + Send + Sync + 'static;
    type Id: Send + Sync + Clone + Hash + Eq + 'static;

    /// Create a new column with the given name
    fn create_column<Type: ColumnType>(&self, name: &str) -> Self::Column<Type>;

    /// Convert a typed column to type-erased column
    fn to_any_column<Type: ColumnType>(
        &self,
        column: Self::Column<Type>,
    ) -> Self::Column<Self::AnyType>;

    /// Attempt to convert a type-erased column back to typed column
    fn convert_any_column<Type: ColumnType>(
        &self,
        any_column: Self::Column<Self::AnyType>,
    ) -> Option<Self::Column<Type>>;

    /// Create an expression from a template and parameters, similar to Expression::new
    fn expr(
        &self,
        template: impl Into<String>,
        parameters: Vec<ExpressiveEnum<Self::Value>>,
    ) -> Expression<Self::Value>;

    /// Create a search expression for a table (e.g., searches across searchable fields)
    ///
    /// Different vendors implement search differently:
    /// - SQL: `field LIKE '%value%'`
    /// - SurrealDB: `field CONTAINS 'value'` or `field ~ 'value'`
    /// - MongoDB: `{ field: { $regex: 'value', $options: 'i' } }`
    ///
    /// The implementation should search across appropriate fields in the table.
    fn search_table_expr(
        &self,
        table: &impl TableLike,
        search_value: &str,
    ) -> Expression<Self::Value>;

    /// Get all data from a table as Record values with IDs (for ReadableValueSet implementation)
    async fn list_table_values<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<IndexMap<Self::Id, Record<Self::Value>>>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Get a single record by ID as Record value (for ReadableValueSet implementation)
    async fn get_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Get some data from a table as Record value with ID (for ReadableValueSet implementation)
    async fn get_table_some_value<E>(
        &self,
        table: &Table<Self, E>,
    ) -> Result<Option<(Self::Id, Record<Self::Value>)>>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Get count of records in the table
    async fn get_count<E>(&self, table: &Table<Self, E>) -> Result<i64>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Get sum of a column in the table
    async fn get_sum<E, Type: ColumnType>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Type>,
    ) -> Result<Type>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Get maximum value of a column in the table
    async fn get_max<E, Type: ColumnType>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Type>,
    ) -> Result<Type>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Get minimum value of a column in the table
    async fn get_min<E, Type: ColumnType>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<Type>,
    ) -> Result<Type>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Insert a record as Record value (for WritableValueSet implementation)
    async fn insert_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Replace a record as Record value (for WritableValueSet implementation)
    async fn replace_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        record: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Patch a record as Record value (for WritableValueSet implementation)
    async fn patch_table_value<E>(
        &self,
        table: &Table<Self, E>,
        id: &Self::Id,
        partial: &Record<Self::Value>,
    ) -> Result<Record<Self::Value>>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Delete a record by ID (for WritableValueSet implementation)
    async fn delete_table_value<E>(&self, table: &Table<Self, E>, id: &Self::Id) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Delete all records (for WritableValueSet implementation)
    async fn delete_table_all_values<E>(&self, table: &Table<Self, E>) -> Result<()>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Insert a record and return generated ID (for InsertableValueSet implementation)
    async fn insert_table_return_id_value<E>(
        &self,
        table: &Table<Self, E>,
        record: &Record<Self::Value>,
    ) -> Result<Self::Id>
    where
        E: Entity<Self::Value>,
        Self: Sized;

    /// Stream all records from a table as (Id, Record) pairs.
    ///
    /// Default implementation wraps `list_table_values` into a stream.
    /// Backends with native streaming (e.g. REST APIs with pagination)
    /// can override this to yield records incrementally.
    #[allow(clippy::type_complexity)]
    fn stream_table_values<'a, E>(
        &'a self,
        table: &Table<Self, E>,
    ) -> Pin<Box<dyn Stream<Item = Result<(Self::Id, Record<Self::Value>)>> + Send + 'a>>
    where
        E: Entity<Self::Value> + 'a,
        Self: Sized,
    {
        let table = table.clone();
        Box::pin(async_stream::stream! {
            let records = self.list_table_values(&table).await;
            match records {
                Ok(map) => {
                    for item in map {
                        yield Ok(item);
                    }
                }
                Err(e) => yield Err(e),
            }
        })
    }

    /// Return an associated expression that, when resolved, yields all values
    /// of the given typed column from this table (respecting current conditions).
    ///
    /// For query-language backends, this can be a subquery expression.
    /// For simple backends (CSV), this uses a `DeferredFn` that loads data
    /// and extracts the column values at execution time.
    ///
    /// The returned `AssociatedExpression` can be:
    /// - Executed directly: `.get().await -> Result<Vec<Type>>`
    /// - Composed into expressions: used via `Expressive` trait in `in_()` conditions
    ///
    /// ```rust,ignore
    /// let fk_col = source.get_column::<String>("bakery_id").unwrap();
    /// let fk_values = source.data_source().column_table_values_expr(&source, &fk_col);
    /// // Execute: let ids = fk_values.get().await?;
    /// // Or compose: target.add_condition(target["id"].in_((fk_values)));
    /// ```
    fn column_table_values_expr<'a, E, Type: ColumnType>(
        &'a self,
        table: &Table<Self, E>,
        column: &Self::Column<Type>,
    ) -> AssociatedExpression<'a, Self, Self::Value, Vec<Type>>
    where
        E: Entity<Self::Value> + 'static,
        Self: ExprDataSource<Self::Value> + Sized;
}
