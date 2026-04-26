//! `ExprDataSource` implementation for redb.
//!
//! redb has no expression engine, so `execute()` only knows how to resolve
//! deferred parameters (used by `column_table_values_expr` and the
//! relationship traversal path). Calling `execute()` on a non-deferred
//! expression returns an error — it's not meant to be a general query
//! interface.

use vantage_expressions::{
    DeferredFn, Expression, ExpressiveEnum, traits::datasource::ExprDataSource,
};

use crate::redb::Redb;
use crate::types::AnyRedbType;

impl ExprDataSource<AnyRedbType> for Redb {
    async fn execute(&self, expr: &Expression<AnyRedbType>) -> vantage_core::Result<AnyRedbType> {
        if expr.parameters.is_empty() {
            return Ok(AnyRedbType::untyped(ciborium::Value::Text(
                expr.template.clone(),
            )));
        }
        if expr.parameters.len() != 1 {
            return Err(vantage_core::error!(
                "Redb does not support multi-parameter expression execution"
            ));
        }
        match &expr.parameters[0] {
            ExpressiveEnum::Scalar(v) => Ok(v.clone()),
            ExpressiveEnum::Deferred(d) => match d.call().await? {
                ExpressiveEnum::Scalar(v) => Ok(v),
                _ => Err(vantage_core::error!(
                    "Deferred resolved to non-scalar in redb execute"
                )),
            },
            // Single nested parameter: pull its scalar / deferred shallowly.
            // We deliberately do not recurse to avoid the async type cycle —
            // redb's internal use of execute() never goes more than one level
            // deep (column_table_values_expr wraps a deferred fn).
            ExpressiveEnum::Nested(inner) => {
                if inner.parameters.len() == 1 {
                    match &inner.parameters[0] {
                        ExpressiveEnum::Scalar(v) => Ok(v.clone()),
                        ExpressiveEnum::Deferred(d) => match d.call().await? {
                            ExpressiveEnum::Scalar(v) => Ok(v),
                            _ => Err(vantage_core::error!(
                                "Nested deferred resolved to non-scalar"
                            )),
                        },
                        ExpressiveEnum::Nested(_) => Err(vantage_core::error!(
                            "Redb execute: only one level of nesting supported"
                        )),
                    }
                } else {
                    Err(vantage_core::error!(
                        "Redb execute: nested expression must have exactly one parameter"
                    ))
                }
            }
        }
    }

    fn defer(&self, expr: Expression<AnyRedbType>) -> DeferredFn<AnyRedbType>
    where
        AnyRedbType: Clone + Send + Sync + 'static,
    {
        let db = self.clone();
        DeferredFn::new(move || {
            let db = db.clone();
            let expr = expr.clone();
            Box::pin(async move {
                let result = db.execute(&expr).await?;
                Ok(ExpressiveEnum::Scalar(result))
            })
        })
    }
}
