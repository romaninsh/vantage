//! Stub ExprDataSource — MongoDB doesn't use Expression-based queries,
//! but this is needed to satisfy the `column_table_values_expr` bound.

use vantage_expressions::{DeferredFn, ExprDataSource, Expression};

use crate::mongodb::MongoDB;
use crate::types::AnyMongoType;

impl ExprDataSource<AnyMongoType> for MongoDB {
    async fn execute(
        &self,
        _expr: &Expression<AnyMongoType>,
    ) -> vantage_core::Result<AnyMongoType> {
        Err(vantage_core::error!(
            "MongoDB does not support Expression-based execution"
        ))
    }

    fn defer(&self, expr: Expression<AnyMongoType>) -> DeferredFn<AnyMongoType> {
        let db = self.clone();
        DeferredFn::from_fn(move || {
            let db = db.clone();
            let expr = expr.clone();
            Box::pin(async move { db.execute(&expr).await })
        })
    }
}
