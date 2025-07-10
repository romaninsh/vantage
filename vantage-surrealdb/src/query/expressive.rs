use async_trait::async_trait;
use vantage_expressions::{Expressive, OwnedExpression, expr, protocol::DataSource};

use crate::query::Query;

#[async_trait]
impl Expressive for Query {
    async fn prepare(&self, _data_source: &dyn DataSource) -> OwnedExpression {
        expr!("QUERY!")
    }
}
