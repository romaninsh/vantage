//! Stub `ExprDataSource` for GraphQL.
//!
//! A GraphQL query is a structured document, not a parameterised template,
//! so `execute` always errors. The impl exists to satisfy the trait bound on
//! `column_table_values_expr`.

use vantage_expressions::{DeferredFn, ExprDataSource, Expression};

use crate::graphql::api::GraphqlApi;
use crate::graphql::types::AnyGraphqlType;

impl ExprDataSource<AnyGraphqlType> for GraphqlApi {
    async fn execute(
        &self,
        _expr: &Expression<AnyGraphqlType>,
    ) -> vantage_core::Result<AnyGraphqlType> {
        Err(vantage_core::error!(
            "GraphqlApi does not support Expression-based execution; use GraphqlSelect"
        ))
    }

    fn defer(&self, expr: Expression<AnyGraphqlType>) -> DeferredFn<AnyGraphqlType> {
        let api = self.clone();
        DeferredFn::from_fn(move || {
            let api = api.clone();
            let expr = expr.clone();
            Box::pin(async move { api.execute(&expr).await })
        })
    }
}
