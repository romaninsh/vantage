use std::fmt::Debug;

use async_trait::async_trait;
use serde_json::Value;

use crate::OwnedExpression;

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

/// There are several expressions provided by this crate - LazyExpression and OwnedExpressions.
/// Also - DataSource vendors will add their own versions of expressions of extended syntax.
///
/// All of those implement an Expressive trait - meaning they can be part of
/// other expressions. Expressive trait can also be applied to other objects,
/// such as "Field" or "Query" allowing those to become part of expression
/// too.
#[async_trait]
pub trait Expressive: Send + Sync + Debug {
    /// All parts of expressions should be able to convert into OwnedExpression
    async fn prepare(&self, data_source: &dyn DataSource) -> OwnedExpression;
}

/// Trait can be used for arguments to pass arbitrary arguments, that can be converted
/// into expression
pub trait IntoExpression {
    fn into_expression(self) -> Box<dyn Expressive>;
}
