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

pub trait SelectSource<Ex = crate::Expression>: DataSource {
    type Select<E>: Selectable<Ex>
    where
        E: crate::Entity;

    // Return SelectQuery with entity type information
    fn select<E>(&self) -> Self::Select<E>
    where
        E: crate::Entity;

    // Execute select query directly
    fn execute_select<E>(&self, select: &Self::Select<E>) -> impl Future<Output = Value> + Send
    where
        E: crate::Entity;
}
