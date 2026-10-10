//! `ExprDataSource` impl for [`KubernetesCluster`].
//!
//! Same shape as `vantage-aws`: `execute` resolves the single-parameter
//! expression that `column_table_values_expr` produces (a Deferred
//! wrapping a column projection), so `with_one` / `with_many` traversal
//! via the subquery path works.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_expressions::{
    Expression, defer_execute, execute_by_resolving, traits::datasource::ExprDataSource,
    traits::expressive::DeferredFn,
};

use crate::cluster::KubernetesCluster;

impl ExprDataSource<CborValue> for KubernetesCluster {
    async fn execute(&self, expr: &Expression<CborValue>) -> Result<CborValue> {
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
