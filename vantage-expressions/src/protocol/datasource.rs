use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

use crate::Selectable;

pub trait DataSource: Send + Sync {}

/// Datasource implements a basic query interface for expression engine T
/// that allow queries to be executed instantly (async) or convert them
/// into closure, that can potentially be used in a different query.
pub trait QuerySource<T>: DataSource {
    fn execute(&self, expr: &T) -> impl Future<Output = Value> + Send;

    fn defer(
        &self,
        expr: T,
    ) -> impl Fn() -> Pin<Box<dyn Future<Output = Value> + Send>> + Send + Sync + 'static;
}

pub trait SelectSource: DataSource {
    type Select: Selectable;

    // Return SelectQuery
    fn select(&self) -> Self::Select;
}
