//! Stub `ExprDataSource` — DynamoDB doesn't run a SQL-style expression
//! engine, but the trait bound on `TableSource::column_table_values_expr`
//! requires the impl to exist.

use vantage_expressions::{DeferredFn, ExprDataSource, Expression};

use crate::dynamodb::DynamoDB;
use crate::dynamodb::types::AnyDynamoType;

impl ExprDataSource<AnyDynamoType> for DynamoDB {
    async fn execute(
        &self,
        _expr: &Expression<AnyDynamoType>,
    ) -> vantage_core::Result<AnyDynamoType> {
        Err(vantage_core::error!(
            "DynamoDB does not support Expression-based execution"
        ))
    }

    fn defer(&self, expr: Expression<AnyDynamoType>) -> DeferredFn<AnyDynamoType> {
        let db = self.clone();
        DeferredFn::from_fn(move || {
            let db = db.clone();
            let expr = expr.clone();
            Box::pin(async move { db.execute(&expr).await })
        })
    }
}
