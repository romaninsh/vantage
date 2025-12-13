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
        self.data_source().get_table_expr_count(self)
        /*
        let table_name = self.table_name().to_string();
        let query = self.data_source.expr(
            "SELECT COUNT(*) FROM {}",
            vec![ExpressiveEnum::Scalar(table_name.into())],
        );
        self.data_source.associate::<i64>(query)
        */
    }

    /// Get an expression for the maximum value of a column
    /// Returns an AssociatedExpression that can be executed or composed
    pub fn get_expr_max<R: ColumnType + Default + AddAssign>(
        &self,
        column: &T::Column<R>,
    ) -> AssociatedExpression<'_, T, T::Value, R> {
        self.data_source().get_table_expr_max(self, column)
    }
}
