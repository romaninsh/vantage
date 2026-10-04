//! `DataSource` + `ExprDataSource` impls for `Cmd`.
//!
//! Same shape as `vantage-aws`: `execute` resolves the single deferred
//! parameter that `column_table_values_expr` produces (the projection
//! used by relation traversal). Nothing else needs a real implementation.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_expressions::{
    Expression, resolve_param,
    traits::datasource::{DataSource, ExprDataSource},
    traits::expressive::{DeferredFn, ExpressiveEnum},
};

use crate::cmd::Cmd;

impl DataSource for Cmd {}

impl ExprDataSource<CborValue> for Cmd {
    async fn execute(&self, expr: &Expression<CborValue>) -> Result<CborValue> {
        if expr.parameters.is_empty() {
            Ok(CborValue::Text(expr.template.clone()))
        } else if expr.parameters.len() == 1 {
            // Collapses a `column_table_values_expr` chain
            // (Nested → Deferred → Scalar) to the projected array.
            resolve_param(&expr.parameters[0], |t| CborValue::Text(t.to_string())).await
        } else {
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
