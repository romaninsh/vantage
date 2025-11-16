use serde_json::Value;
use std::future::Future;

use crate::Selectable;
use crate::traits::expressive::DeferredFn;
use vantage_core::Result;

pub trait DataSource: Send + Sync {}

/// Datasource implements a basic query interface for expression engine T
/// that allow queries to be executed instantly (async) or convert them
/// into closure, that can potentially be used in a different query.
pub trait QuerySource<T = Value>: DataSource {
    fn execute(&self, expr: &crate::Expression<T>) -> impl Future<Output = Result<T>> + Send;

    fn defer(&self, expr: crate::Expression<T>) -> DeferredFn<T>
    where
        T: Clone + Send + Sync + 'static;
}

pub trait SelectSource<T = Value>: DataSource {
    type Select: Selectable<T>;

    // Return SelectQuery
    fn select(&self) -> Self::Select;

    // Execute select query directly
    fn execute_select(&self, select: &Self::Select) -> impl Future<Output = Result<Vec<T>>> + Send;
}
