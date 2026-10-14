//! `ExprDataSource` for CSV: executing an expression resolves its deferred
//! parameters, which is how `column_table_values_expr` yields its list.

use vantage_expressions::traits::datasource::ExprDataSource;
use vantage_expressions::traits::expressive::DeferredFn;
use vantage_expressions::{Expression, defer_execute, execute_by_resolving};

use crate::Csv;
use crate::condition::resolve_param;
use crate::type_system::AnyCsvType;

impl ExprDataSource<AnyCsvType> for Csv {
    async fn execute(&self, expr: &Expression<AnyCsvType>) -> vantage_core::Result<AnyCsvType> {
        // Multi-param expressions resolve each param and join the values
        // comma-separated.
        execute_by_resolving(
            expr,
            |template| AnyCsvType::new(template.to_string()),
            |params| async move {
                let mut results = Vec::with_capacity(params.len());
                for param in params {
                    results.push(resolve_param(param).await?.value().clone());
                }
                Ok(AnyCsvType::new(results.join(",")))
            },
        )
        .await
    }

    fn defer(&self, expr: Expression<AnyCsvType>) -> DeferredFn<AnyCsvType>
    where
        AnyCsvType: Clone + Send + Sync + 'static,
    {
        defer_execute(self, expr)
    }
}
