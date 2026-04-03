use std::ops::AddAssign;

use serde_json::Value;
use vantage_expressions::{
    AssociatedExpression, ExprDataSource, Expression, traits::datasource::DataSource,
};
use vantage_types::Entity;

use crate::{column::core::ColumnType, prelude::TableSource, table::Table};

/// Trait for table data sources that can return composable expressions for aggregation.
///
/// Unlike `TableSource` methods (which execute and return values), these methods return
/// `AssociatedExpression` that can be:
/// - Executed directly: `.get().await -> Result<T>`
/// - Composed into other queries as subexpressions
pub trait TableExprSource<Ex = Expression<Value>>:
    DataSource + TableSource + ExprDataSource<Self::Value>
{
    /// Return an expression that resolves to the count of records in the table
    fn get_table_count_expr<E>(
        &self,
        table: &Table<Self, E>,
    ) -> AssociatedExpression<'_, Self, Self::Value, usize>
    where
        E: Entity<Self::Value>;

    /// Return an expression that resolves to the sum of a column
    fn get_table_sum_expr<E, R>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> AssociatedExpression<'_, Self, Self::Value, R>
    where
        R: ColumnType + Default + AddAssign,
        E: Entity<Self::Value>;

    /// Return an expression that resolves to the maximum value of a column
    fn get_table_max_expr<E, R>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> AssociatedExpression<'_, Self, Self::Value, R>
    where
        R: ColumnType + Default + AddAssign,
        E: Entity<Self::Value>;

    /// Return an expression that resolves to the minimum value of a column
    fn get_table_min_expr<E, R>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> AssociatedExpression<'_, Self, Self::Value, R>
    where
        R: ColumnType + Default + AddAssign,
        E: Entity<Self::Value>;
}
