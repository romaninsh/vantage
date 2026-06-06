//! `DataSource` + `ExprDataSource` impls for `Cmd`.
//!
//! Same shape as `vantage-aws`: `execute` resolves the single deferred
//! parameter that `column_table_values_expr` produces (the projection
//! used by relation traversal). Nothing else needs a real implementation.

use std::future::Future;
use std::pin::Pin;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_expressions::{
    Expression,
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
            resolve_param(&expr.parameters[0]).await
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

/// Recursively unwrap an `ExpressiveEnum` into its underlying value —
/// mirrors `vantage-csv` / `vantage-aws` so a `column_table_values_expr`
/// chain (Nested → Deferred → Scalar) collapses to the projected array.
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
