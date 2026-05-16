//! Stub `ExprDataSource` for GraphQL.
//!
//! GraphQL doesn't use Expression-based queries — its query language is
//! a structured document, not a parameterised template — so this impl
//! exists only to satisfy the trait bound on `column_table_values_expr`
//! later. Same shape as `vantage-mongodb`'s equivalent stub.

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn execute_returns_error_stub() {
        let api = GraphqlApi::new("https://example.test/graphql");
        let expr = Expression::<AnyGraphqlType>::new("ignored", vec![]);
        let err = api.execute(&expr).await.unwrap_err();
        assert!(
            err.to_string().contains("GraphqlSelect"),
            "stub error should point users at the query builder, got: {err}"
        );
    }
}
