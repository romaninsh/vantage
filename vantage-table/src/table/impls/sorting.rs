use vantage_core::{Result, error};
use vantage_types::Entity;

use crate::{sorting::*, table::Table, traits::table_source::TableSource};

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// Add a permanent order clause
    pub fn add_order(&mut self, order: OrderBy<T::Condition>) {
        let id = -self.next_order_id;
        self.next_order_id += 1;
        self.order_by
            .insert(id, (order.expression, order.direction));
    }

    /// Add a temporary order clause that can be removed later
    pub fn temp_add_order(&mut self, order: OrderBy<T::Condition>) -> OrderHandle {
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
    pub fn orders(&self) -> impl Iterator<Item = &(T::Condition, SortDirection)> {
        self.order_by.values()
    }

    /// Add an order clause using the builder pattern
    pub fn with_order(mut self, order: OrderBy<T::Condition>) -> Self {
        self.add_order(order);
        self
    }
}

/// Extension trait for creating OrderBy from expressions and columns
pub trait OrderByExt<C> {
    /// Create an ascending order specification
    fn ascending(&self) -> OrderBy<C>;

    /// Create a descending order specification
    fn descending(&self) -> OrderBy<C>;
}

// Direct implementation for any Clone type (Expression, bson::Document, etc.)
impl<C: Clone> OrderByExt<C> for C {
    fn ascending(&self) -> OrderBy<C> {
        OrderBy::ascending(self.clone())
    }

    fn descending(&self) -> OrderBy<C> {
        OrderBy::descending(self.clone())
    }
}
