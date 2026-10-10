//! `DataSource` + `ExprDataSource` impls for `Cmd`.
//!
//! Same shape as `vantage-aws`: `execute` resolves the single deferred
//! parameter that `column_table_values_expr` produces (the projection
//! used by relation traversal). Nothing else needs a real implementation.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_expressions::{
    Expression, defer_execute, execute_by_resolving,
    traits::datasource::{DataSource, ExprDataSource},
    traits::expressive::DeferredFn,
};

use crate::cmd::Cmd;

impl DataSource for Cmd {}

impl ExprDataSource<CborValue> for Cmd {
    async fn execute(&self, expr: &Expression<CborValue>) -> Result<CborValue> {
        // A single parameter is a `column_table_values_expr` chain
        // (Nested → Deferred → Scalar) and collapses to the projected array.
        execute_by_resolving(
            expr,
            |t| CborValue::Text(t.to_string()),
            |_| async { Ok(CborValue::Null) },
        )
        .await
    }

    fn defer(&self, expr: Expression<CborValue>) -> DeferredFn<CborValue> {
        defer_execute(self, expr)
    }
}
