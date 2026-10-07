//! `ExprDataSource` impl for `AwsAccount`.
//!
//! `execute` resolves an expression's single parameter — the shape
//! `column_table_values_expr` produces (one Deferred parameter wrapping
//! a column projection). Multi-parameter expressions aren't a thing for
//! this backend, so they fall back to a stable null/template result for
//! the relationship machinery to tolerate without panicking.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_expressions::{
    Expression, defer_execute, execute_by_resolving, traits::datasource::ExprDataSource,
    traits::expressive::DeferredFn,
};

use crate::account::AwsAccount;

impl ExprDataSource<CborValue> for AwsAccount {
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
