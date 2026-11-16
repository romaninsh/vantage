use std::ops::AddAssign;

use vantage_expressions::Expressive;
use vantage_expressions::expr_any;
use vantage_expressions::traits::associated_expressions::AssociatedExpression;
use vantage_table::column::core::ColumnType;
use vantage_table::table::Table;
use vantage_table::traits::table_expr_source::TableExprSource;
use vantage_types::Entity;

use crate::surrealdb::SurrealDB;
use crate::{AnySurrealType, Expr};

/// What an aggregate reads for the column named `name`: its expression when
/// it is an expression column of `table`, the field otherwise. No other
/// callback runs. Panics if that callback fails: these builders return no
/// `Result`.
fn value_of<E: Entity<AnySurrealType>>(table: &Table<SurrealDB, E>, name: &str) -> Expr {
    table
        .try_expression_column(name)
        .unwrap_or_else(|e| panic!("{e}"))
        .unwrap_or_else(|| expr_any!(name.to_string()))
}

impl TableExprSource for SurrealDB {
    fn get_table_count_expr<E>(
        &self,
        table: &Table<Self, E>,
    ) -> AssociatedExpression<'_, Self, Self::Value, usize>
    where
        E: Entity<Self::Value>,
    {
        let mut select = table.select_empty();
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
        let mut select = table.select_empty();
        select.order_by.clear();
        let sum_return = select.as_sum(value_of(table, column.name()));
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
        let mut select = table.select_empty();
        select.order_by.clear();
        let max_return = select.as_max(value_of(table, column.name()));
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
        let mut select = table.select_empty();
        select.order_by.clear();
        let min_return = select.as_min(value_of(table, column.name()));
        AssociatedExpression::new(min_return.expr(), self)
    }
}
