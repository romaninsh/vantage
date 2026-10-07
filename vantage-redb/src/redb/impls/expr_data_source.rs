//! `ExprDataSource` implementation for redb.
//!
//! redb has no expression engine, so `execute()` only knows how to resolve
//! deferred parameters (used by `column_table_values_expr` and the
//! relationship traversal path), one level of nesting deep. Any other
//! expression shape returns an error — it's not meant to be a general query
//! interface.

use vantage_expressions::{
    DeferredFn, Expression, defer_execute, execute_strict, traits::datasource::ExprDataSource,
};

use crate::redb::Redb;
use crate::types::AnyRedbType;

impl ExprDataSource<AnyRedbType> for Redb {
    async fn execute(&self, expr: &Expression<AnyRedbType>) -> vantage_core::Result<AnyRedbType> {
        execute_strict(
            expr,
            |template| AnyRedbType::untyped(ciborium::Value::Text(template.to_string())),
            "Redb",
        )
        .await
    }

    fn defer(&self, expr: Expression<AnyRedbType>) -> DeferredFn<AnyRedbType>
    where
        AnyRedbType: Clone + Send + Sync + 'static,
    {
        defer_execute(self, expr)
    }
}
