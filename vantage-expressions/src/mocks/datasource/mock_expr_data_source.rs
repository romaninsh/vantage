//! `MockExprDataSource`: an `ExprDataSource` that answers every query with one value.

use crate::traits::datasource::{DataSource, ExprDataSource};
use crate::traits::expressive::{DeferredFn, ExpressiveEnum};
use serde_json::Value;
use vantage_core::Result;

/// An `ExprDataSource` that returns the same value for every expression.
#[derive(Debug, Clone)]
pub struct MockExprDataSource {
    value: Value,
}

impl MockExprDataSource {
    /// A source that returns `value` for every expression.
    pub fn new(value: Value) -> Self {
        Self { value }
    }
}

impl DataSource for MockExprDataSource {}

impl ExprDataSource<serde_json::Value> for MockExprDataSource {
    async fn execute(
        &self,
        _expr: &crate::Expression<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        Ok(self.value.clone())
    }

    fn defer(&self, _expr: crate::Expression<serde_json::Value>) -> DeferredFn<serde_json::Value>
    where
        serde_json::Value: Clone + Send + Sync + 'static,
    {
        let value = self.value.clone();
        DeferredFn::new(move || {
            let value = value.clone();
            Box::pin(async move { Ok(ExpressiveEnum::Scalar(value)) })
        })
    }
}
