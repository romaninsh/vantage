use ciborium::Value;
use vantage_core::{Context, Result, error};
use vantage_expressions::{DeferredFn, ExprDataSource, Expression};

use crate::{AnySurrealType, SurrealType, surrealdb::SurrealDB};

impl ExprDataSource<AnySurrealType> for SurrealDB {
    async fn execute(&self, expr: &Expression<AnySurrealType>) -> Result<AnySurrealType> {
        let (query_str, params) = self.prepare_query(expr);
        let params_cbor = params.to_cbor();
        let client = self.inner.lock().await;
        let result = client
            .query_cbor(&query_str, params_cbor)
            .await
            .context("Executing SurrealDB query")?;

        AnySurrealType::from_cbor(&result)
            .ok_or(error!("SurrealDB query successful, but returned no resust"))
    }

    fn defer(&self, expr: Expression<AnySurrealType>) -> DeferredFn<AnySurrealType> {
        let client = self.clone();
        DeferredFn::from_fn(move || {
            let client = client.clone();
            let expr = expr.clone();
            Box::pin(async move { client.execute(&expr).await })
        })
    }
}
