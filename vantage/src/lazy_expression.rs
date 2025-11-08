use core::fmt;
use std::sync::Arc;

use serde_json::Value;

use crate::{
    prelude::{Expression, Table},
    traits::{datasource::DataSource, entity::Entity},
};

/// Represents a lazily evaluated expression that can be executed either before or after a query
/// Generic parameters:
/// - T: The data source type implementing DataSource trait
/// - E: The entity type implementing Entity trait
#[derive(Clone)]
pub enum LazyExpression<T: DataSource, E: Entity> {
    /// Transforms the query result after it has been fetched
    /// Contains a thread-safe function that takes a JSON Value and returns a transformed Value
    AfterQuery(Arc<Box<dyn Fn(&Value) -> Value + Send + Sync + 'static>>),
    /// Modifies the query expression before it is executed
    /// Contains a thread-safe function that takes a Table and returns an Expression
    BeforeQuery(Arc<Box<dyn Fn(&Table<T, E>) -> Expression + Send + Sync + 'static>>),
}

/// Implements Debug formatting for LazyExpression
/// Since closures can't be formatted directly, it provides a simple string representation
impl<T: DataSource, E: Entity> fmt::Debug for LazyExpression<T, E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LazyExpression::AfterQuery(_) => f.write_str("AfterQuery(<closure>)"),
            LazyExpression::BeforeQuery(_) => f.write_str("BeforeQuery(<closure>)"),
        }
    }
}
