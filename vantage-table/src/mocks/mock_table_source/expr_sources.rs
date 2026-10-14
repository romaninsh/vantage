//! The expression and select sources the mock delegates to, and the
//! aggregate expressions it builds.

use serde_json::Value;
use vantage_expressions::{
    Expression, expr_any,
    mocks::select::MockSelect,
    traits::datasource::{ExprDataSource, SelectableDataSource},
    traits::expressive::{DeferredFn, ExpressiveEnum},
};
use vantage_types::Entity;

use super::MockTableSource;
use crate::column::core::ColumnType;
use crate::{
    table::Table,
    traits::{column_like::ColumnLike, table_expr_source::TableExprSource},
};

impl ExprDataSource<Value> for MockTableSource {
    async fn execute(&self, expr: &Expression<Value>) -> vantage_core::Result<Value> {
        if let Some(ref query_source) = self.query_source {
            let source = {
                let guard = query_source.lock().unwrap();
                guard.clone()
            };
            source.execute(expr).await
        } else {
            panic!("MockTableSource query source not set. Use with_query_source() to configure it.")
        }
    }

    fn defer(
        &self,
        expr: Expression<Value>,
    ) -> vantage_expressions::traits::expressive::DeferredFn<Value>
    where
        Value: Clone + Send + Sync + 'static,
    {
        if let Some(ref query_source) = self.query_source {
            let query_source_clone = query_source.clone();
            let expr_clone = expr.clone();
            DeferredFn::new(move || {
                let query_source = query_source_clone.clone();
                let expr = expr_clone.clone();
                Box::pin(async move {
                    let source = {
                        let guard = query_source.lock().unwrap();
                        guard.clone()
                    };
                    match source.execute(&expr).await {
                        Ok(value) => Ok(ExpressiveEnum::Scalar(value)),
                        Err(e) => Err(e),
                    }
                })
            })
        } else {
            panic!("MockTableSource query source not set. Use with_query_source() to configure it.")
        }
    }
}

impl SelectableDataSource<Value> for MockTableSource {
    type Select = MockSelect;

    fn select(&self) -> Self::Select {
        if let Some(ref select_source) = self.select_source {
            select_source.select()
        } else {
            panic!(
                "MockTableSource select source not set. Use with_select_source() to configure it."
            )
        }
    }

    async fn execute_select(&self, select: &Self::Select) -> vantage_core::Result<Vec<Value>> {
        if let Some(ref select_source) = self.select_source {
            select_source.execute_select(select).await
        } else {
            panic!(
                "MockTableSource select source not set. Use with_select_source() to configure it."
            )
        }
    }
}

impl TableExprSource for MockTableSource {
    fn get_table_count_expr<E: Entity<Self::Value>>(
        &self,
        table: &Table<Self, E>,
    ) -> vantage_expressions::AssociatedExpression<'_, Self, Self::Value, usize> {
        let table_name = table.table_name();

        // Read the count synchronously from the single store — no block_on, no
        // separate count cache to go stale.
        let count = self.im_data_source.table_len(table_name);

        let expr = expr_any!("select count() from {}", table_name);

        // Seed the query source so executing `expr` resolves to this count.
        // The key must be the expression's rendered form (what the mock matches
        // on at execute time), not a hand-built string.
        if let Some(ref query_source) = self.query_source {
            let mut source = query_source.lock().unwrap();
            *source = source
                .clone()
                .on_exact_select(expr.preview(), serde_json::json!(count));
        }

        vantage_expressions::AssociatedExpression::new(expr, self)
    }

    fn get_table_sum_expr<E, R>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> vantage_expressions::AssociatedExpression<'_, Self, Self::Value, R>
    where
        E: Entity<Self::Value>,
        R: ColumnType + Default + std::ops::AddAssign,
    {
        let column_name = column.name();
        let table_name = table.table_name();
        let expr = expr_any!("select sum({}) from {}", column_name, table_name);
        vantage_expressions::AssociatedExpression::new(expr, self)
    }

    fn get_table_max_expr<E: Entity<Self::Value>, R: ColumnType>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> vantage_expressions::AssociatedExpression<'_, Self, Self::Value, R> {
        let column_name = column.name();
        let table_name = table.table_name();
        let expr = expr_any!("select max({}) from {}", column_name, table_name);
        vantage_expressions::AssociatedExpression::new(expr, self)
    }

    fn get_table_min_expr<E: Entity<Self::Value>, R: ColumnType>(
        &self,
        table: &Table<Self, E>,
        column: &Self::Column<R>,
    ) -> vantage_expressions::AssociatedExpression<'_, Self, Self::Value, R> {
        let column_name = column.name();
        let table_name = table.table_name();
        let expr = expr_any!("select min({}) from {}", column_name, table_name);
        vantage_expressions::AssociatedExpression::new(expr, self)
    }
}
