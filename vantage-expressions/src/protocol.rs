pub mod expressive;
pub mod result;
pub mod selectable;

use std::fmt::Debug;

use async_trait::async_trait;
use serde_json::Value;

/// DataSource is implemented by vantage-sql, vantage-surrealdb and vantage-graphql
/// but can also be extended by 3rd party persistence vendors, if the persistence
/// allows use of expressions
#[async_trait]
pub trait DataSource: Send + Sync {
    // async fn prepare_expression (&self, )
}

/// We rely on Value for storing simple scalar values. Anything that can be turned
/// into Value async should be implementing PreparableValue. DataSource will be
/// provided in order to prepare value. Examples could be a sub-query.
#[async_trait]
pub trait PreparableValue: Send + Sync + Debug {
    async fn into_value(&self, data_source: &dyn DataSource) -> Value;
}
