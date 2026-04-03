use std::ops::AddAssign;

use vantage_expressions::Expressive;
use vantage_expressions::expr_any;
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_table::column::core::ColumnType;
use vantage_table::table::Table;
use vantage_table::traits::table_expr_source::TableExprSource;
use vantage_types::Entity;

use crate::surrealdb::SurrealDB;

impl TableExprSource for SurrealDB {
    fn get_table_count_expr<E>(
        &self,
        table: &Table<Self, E>,
    ) -> AssociatedExpression<'_, Self, Self::Value, usize>
    where
        E: Entity<Self::Value>,
    {
        let mut select = table.select();
        select.order_by.clear();
        let count_return = select.as_count();
        AssociatedExpression::new(count_return.expr(), self)
    }

    fn get_table_sum_expr<E, R>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> AssociatedExpression<'_, Self, Self::Value, R>
    where
        R: ColumnType + Default + AddAssign,
        E: Entity<Self::Value>,
    {
        let mut select = table.select();
        select.order_by.clear();
        let col_expr = expr_any!(column.name().to_string());
        let sum_return = select.as_sum(col_expr);
        AssociatedExpression::new(sum_return.expr(), self)
    }

    fn get_table_max_expr<E, R>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> AssociatedExpression<'_, Self, Self::Value, R>
    where
        R: ColumnType,
        E: Entity<Self::Value>,
    {
        let mut select = table.select();
        select.order_by.clear();
        let col_expr = expr_any!(column.name().to_string());
        let max_return = select.as_max(col_expr);
        AssociatedExpression::new(max_return.expr(), self)
    }

    fn get_table_min_expr<E, R>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> AssociatedExpression<'_, Self, Self::Value, R>
    where
        R: ColumnType,
        E: Entity<Self::Value>,
    {
        let mut select = table.select();
        select.order_by.clear();
        let col_expr = expr_any!(column.name().to_string());
        let min_return = select.as_min(col_expr);
        AssociatedExpression::new(min_return.expr(), self)
    }
}
