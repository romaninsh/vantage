use serde_json::Value;
use std::future::Future;

use crate::Selectable;
use crate::traits::associated_expressions::AssociatedExpression;
use crate::traits::expressive::DeferredFn;
use vantage_core::Result;

/// DataSource can be referenced by other objects, and will help associate them
/// with physical persistence. While DataSource might not be directly used,
/// It is extended by varietty of other traits that provide
/// real interface (like `QuerySource` and `SelectSource`)
pub trait DataSource: Send + Sync {}

/// DataSource that can also execute expressions.
pub trait ExprDataSource<T = Value>: DataSource {
    fn execute(&self, expr: &crate::Expression<T>) -> impl Future<Output = Result<T>> + Send;

    fn defer(&self, expr: crate::Expression<T>) -> DeferredFn<T>
    where
        T: Clone + Send + Sync + 'static;

    /// Create an associated expression with type-safe return type
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// let count_expr = expr!("SELECT COUNT(*) FROM users");
    /// let associated = ds.associate::<usize>(count_expr);
    /// let result: usize = associated.get().await?;
    /// ```
    fn associate<R>(&self, expr: crate::Expression<T>) -> AssociatedExpression<'_, Self, T, R>
    where
        Self: Sized,
    {
        AssociatedExpression::new(expr, self)
    }
}

/// Datasource that support creation and execution of select queries
pub trait SelectableDataSource<T = Value>: DataSource {
    type Select: Selectable<T>;

    // Return SelectQuery
    fn select(&self) -> Self::Select;

    // Execute select query directly
    fn execute_select(&self, select: &Self::Select) -> impl Future<Output = Result<Vec<T>>> + Send;
}
