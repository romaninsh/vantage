//! `ExprDataSource` impl for `AwsJson1`.
//!
//! `execute` resolves an expression's single parameter — the shape
//! `column_table_values_expr` produces (one Deferred parameter wrapping
//! a column projection). Multi-parameter expressions aren't a thing for
//! this backend, so they fall back to a stable null/template result for
//! the relationship machinery to tolerate without panicking.

use std::future::Future;
use std::pin::Pin;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_expressions::{
    Expression,
    traits::datasource::ExprDataSource,
    traits::expressive::{DeferredFn, ExpressiveEnum},
};

use crate::json1::AwsJson1;

impl ExprDataSource<CborValue> for AwsJson1 {
    async fn execute(&self, expr: &Expression<CborValue>) -> Result<CborValue> {
        if expr.parameters.is_empty() {
            Ok(CborValue::Text(expr.template.clone()))
        } else if expr.parameters.len() == 1 {
            resolve_param(&expr.parameters[0]).await
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

/// Recursively unwrap an `ExpressiveEnum` into the underlying value.
/// Same shape as `vantage-csv`'s `resolve_param` — needed so a
/// `column_table_values_expr` chain (Nested → Deferred → Scalar)
/// collapses to the projected `CborValue::Array`.
pub(crate) fn resolve_param(
    param: &ExpressiveEnum<CborValue>,
) -> Pin<Box<dyn Future<Output = Result<CborValue>> + Send + '_>> {
    Box::pin(async move {
        match param {
            ExpressiveEnum::Scalar(v) => Ok(v.clone()),
            ExpressiveEnum::Deferred(deferred) => {
                let result = deferred.call().await?;
                match result {
                    ExpressiveEnum::Scalar(v) => Ok(v),
                    other => resolve_param(&other).await,
                }
            }
            ExpressiveEnum::Nested(expr) => {
                if expr.parameters.is_empty() {
                    Ok(CborValue::Text(expr.template.clone()))
                } else if expr.parameters.len() == 1 {
                    resolve_param(&expr.parameters[0]).await
                } else {
                    Ok(CborValue::Text(expr.template.clone()))
                }
            }
        }
    })
}
