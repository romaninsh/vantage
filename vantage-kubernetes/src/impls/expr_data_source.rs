//! `ExprDataSource` impl for [`KubernetesCluster`].
//!
//! Same shape as `vantage-aws`: `execute` resolves the single-parameter
//! expression that `column_table_values_expr` produces (a Deferred
//! wrapping a column projection), so `with_one` / `with_many` traversal
//! via the subquery path works.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_expressions::{
    Expression, resolve_param,
    traits::datasource::ExprDataSource,
    traits::expressive::{DeferredFn, ExpressiveEnum},
};

use crate::cluster::KubernetesCluster;

impl ExprDataSource<CborValue> for KubernetesCluster {
    async fn execute(&self, expr: &Expression<CborValue>) -> Result<CborValue> {
        if expr.parameters.is_empty() {
            Ok(CborValue::Text(expr.template.clone()))
        } else if expr.parameters.len() == 1 {
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
