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
    Expression, resolve_param,
    traits::datasource::ExprDataSource,
    traits::expressive::{DeferredFn, ExpressiveEnum},
};

use crate::account::AwsAccount;

impl ExprDataSource<CborValue> for AwsAccount {
    async fn execute(&self, expr: &Expression<CborValue>) -> Result<CborValue> {
        if expr.parameters.is_empty() {
            Ok(CborValue::Text(expr.template.clone()))
        } else if expr.parameters.len() == 1 {
            // Collapses a `column_table_values_expr` chain
            // (Nested → Deferred → Scalar) to the projected array.
            resolve_param(&expr.parameters[0], |t| CborValue::Text(t.to_string())).await
        } else {
            // Not a shape this backend produces; surface stably so the
            // relationship machinery isn't tripped by trivial probes.
            Ok(CborValue::Null)
        }
    }

    fn defer(&self, expr: Expression<CborValue>) -> DeferredFn<CborValue> {
        let this = self.clone();
        DeferredFn::new(move || {
            let this = this.clone();
            let expr = expr.clone();
            Box::pin(async move {
                let result = ExprDataSource::execute(&this, &expr).await?;
                Ok(ExpressiveEnum::Scalar(result))
            })
        })
    }
}
