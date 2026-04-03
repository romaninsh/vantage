//! Expression-related implementations for Table
//!
//! This module provides methods for creating AssociatedExpressions that can be
//! both executed directly and composed into larger expressions.

use std::ops::AddAssign;

use vantage_expressions::{AssociatedExpression, traits::datasource::ExprDataSource};
use vantage_types::Entity;

use crate::{
    column::core::ColumnType,
    table::Table,
    traits::{table_expr_source::TableExprSource, table_source::TableSource},
};

impl<T, E> Table<T, E>
where
    T: TableSource + ExprDataSource<T::Value> + TableExprSource,
    E: Entity<T::Value>,
{
    /// Get an expression for counting rows in this table
    /// Returns an AssociatedExpression that can be executed or composed
    pub fn get_expr_count(&self) -> AssociatedExpression<'_, T, T::Value, usize>
    where
        T::Value: From<String>,
    {
        self.data_source().get_table_count_expr(self)
    }

    /// Get an expression for the sum of a column
    /// Returns an AssociatedExpression that can be executed or composed
    pub fn get_expr_sum<R: ColumnType + Default + AddAssign>(
        &self,
        column: &T::Column<R>,
    ) -> AssociatedExpression<'_, T, T::Value, R> {
        self.data_source().get_table_sum_expr(self, column)
    }

    /// Get an expression for the maximum value of a column
    /// Returns an AssociatedExpression that can be executed or composed
    pub fn get_expr_max<R: ColumnType + Default + AddAssign>(
        &self,
        column: &T::Column<R>,
    ) -> AssociatedExpression<'_, T, T::Value, R> {
        self.data_source().get_table_max_expr(self, column)
    }

    /// Get an expression for the minimum value of a column
    /// Returns an AssociatedExpression that can be executed or composed
    pub fn get_expr_min<R: ColumnType + Default + AddAssign>(
        &self,
        column: &T::Column<R>,
    ) -> AssociatedExpression<'_, T, T::Value, R> {
        self.data_source().get_table_min_expr(self, column)
    }
}
