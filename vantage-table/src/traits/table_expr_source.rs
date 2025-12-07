use std::ops::AddAssign;

use serde_json::Value;
use vantage_expressions::{
    AssociatedExpression, ExprDataSource, Expression, traits::datasource::DataSource,
};
use vantage_types::Entity;

use crate::{column::column::ColumnType, prelude::TableSource, table::Table};

/// Trait for table data sources that defines column type separate from execution
/// TableSource represents a data source that can create and manage tables
pub trait TableExprSource<Ex = Expression<Value>>:
    DataSource + TableSource + ExprDataSource<Self::Value>
{
    /// Get a select query for all data from a table (for ReadableValueSet implementation)
    fn get_table_expr_count<E>(
        &self,
        table: &Table<Self, E>,
    ) -> AssociatedExpression<'_, Self, Self::Value, usize>
    where
        E: Entity<Self::Value>;

    /// Get a MAX query for a specific column with generic return type
    /// The column type R is determined by the Column<R> parameter
    fn get_table_expr_max<E, R>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> AssociatedExpression<'_, Self, Self::Value, R>
    where
        R: ColumnType + Default + AddAssign,
        E: Entity<Self::Value>;

    // /// Get an insert query for a record into a table (for InsertableValueSet implementation)
    // fn get_table_insert_query<E: Entity<Self::Value>>(
    //     &self,
    //     table: &Table<Self, E>,
    //     record: &vantage_types::Record<Self::Value>,
    // ) -> Result<Self::Insert>;
}
