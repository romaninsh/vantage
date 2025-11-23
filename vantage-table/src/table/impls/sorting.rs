use vantage_core::{Result, error};
use vantage_expressions::Expression;
use vantage_types::Entity;

use crate::{sorting::*, table::Table, traits::table_source::TableSource};

impl<T: TableSource, E: Entity> Table<T, E> {
    /// Add a permanent order clause
    pub fn add_order(&mut self, order: OrderBy<Expression<T::Value>>) {
        let id = -self.next_order_id;
        self.next_order_id += 1;
        self.order_by
            .insert(id, (order.expression, order.direction));
    }

    /// Add a temporary order clause that can be removed later
    pub fn temp_add_order(&mut self, order: OrderBy<Expression<T::Value>>) -> OrderHandle {
        let id = self.next_order_id;
        self.next_order_id += 1;
        self.order_by
            .insert(id, (order.expression, order.direction));
        OrderHandle(id)
    }

    /// Remove a temporary order clause by its handle
    ///
    /// # Errors
    ///
    /// Returns error if the handle refers to a permanent order (added via `add_order`)
    pub fn temp_remove_order(&mut self, handle: OrderHandle) -> Result<()> {
        if handle.0 <= 0 {
            return Err(error!("Cannot remove permanent order"));
        }
        self.order_by.shift_remove(&handle.0);
        Ok(())
    }

    /// Get all order clauses
    pub fn orders(&self) -> impl Iterator<Item = &(Expression<T::Value>, SortDirection)> {
        self.order_by.values()
    }

    /// Add an order clause using the builder pattern
    pub fn with_order(mut self, order: OrderBy<Expression<T::Value>>) -> Self {
        self.add_order(order);
        self
    }
}

/// Extension trait for creating OrderBy from expressions and columns
pub trait OrderByExt<V> {
    /// Create an ascending order specification
    fn ascending(&self) -> OrderBy<Expression<V>>;

    /// Create a descending order specification
    fn descending(&self) -> OrderBy<Expression<V>>;
}

// Note: ColumnLike implementation removed since ColumnLike doesn't have expr() method
// Users should create expressions from columns using other means

// Direct implementation for Expression
impl<T: Clone + Send + Sync + 'static> OrderByExt<T> for Expression<T> {
    fn ascending(&self) -> OrderBy<Expression<T>> {
        OrderBy::ascending(self.clone())
    }

    fn descending(&self) -> OrderBy<Expression<T>> {
        OrderBy::descending(self.clone())
    }
}
