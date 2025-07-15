use async_trait::async_trait;
use vantage_expressions::{Expressive, OwnedExpression, protocol::DataSource};

use crate::query::MongoSelect;

#[async_trait]
impl Expressive for MongoSelect {
    async fn prepare(&self, _data_source: &dyn DataSource) -> OwnedExpression {
        self.clone().into()
    }
}
