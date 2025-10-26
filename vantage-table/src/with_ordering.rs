use super::{Entity, Table, TableSource};
use vantage_core::{Result, error};

/// Sort direction for ordering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// Handle for temporary order clauses that can be removed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderHandle(i64);

impl<T: TableSource, E: Entity> Table<T, E> {
    /// Add a permanent order clause
    pub fn add_order(&mut self, expression: T::Expr, direction: SortDirection) {
        let id = -self.next_order_id;
        self.next_order_id += 1;
        self.order_by.insert(id, (expression, direction));
    }

    /// Add a temporary order clause that can be removed later
    pub fn temp_add_order(&mut self, expression: T::Expr, direction: SortDirection) -> OrderHandle {
        let id = self.next_order_id;
        self.next_order_id += 1;
        self.order_by.insert(id, (expression, direction));
        OrderHandle(id)
    }

    /// Remove a temporary order clause by its handle
    ///
    /// # Errors
    ///
    /// Returns error if the handle refers to a permanent order (added via `add_order`)
    pub fn temp_remove_order(&mut self, handle: OrderHandle) -> Result<()> {
        if handle.0 <= 0 {
            return Err(error!("Cannot remove permanent order").into());
        }
        self.order_by.shift_remove(&handle.0);
        Ok(())
    }

    /// Get all order clauses
    pub fn orders(&self) -> impl Iterator<Item = &(T::Expr, SortDirection)> {
        self.order_by.values()
    }

    /// Add an order clause using the builder pattern
    pub fn with_order(mut self, expression: T::Expr, direction: SortDirection) -> Self {
        self.add_order(expression, direction);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EmptyEntity;
    use crate::mocks::MockTableSource;
    use vantage_expressions::expr;

    #[test]
    fn test_temp_orders() {
        let ds = MockTableSource::new();
        let mut table = Table::<_, EmptyEntity>::new("test", ds);

        // Add permanent order
        table.add_order(expr!("name"), SortDirection::Ascending);
        assert_eq!(table.orders().count(), 1);

        // Add temp orders
        let handle1 = table.temp_add_order(expr!("age"), SortDirection::Descending);
        let handle2 = table.temp_add_order(expr!("created_at"), SortDirection::Ascending);
        assert_eq!(table.orders().count(), 3);

        // Remove one temp order
        table.temp_remove_order(handle1).unwrap();
        assert_eq!(table.orders().count(), 2);

        // Add another permanent
        table.add_order(expr!("id"), SortDirection::Ascending);
        assert_eq!(table.orders().count(), 3);

        // Remove second temp
        table.temp_remove_order(handle2).unwrap();
        assert_eq!(table.orders().count(), 2);

        // Verify we have exactly 2 orders left (both permanent)
        assert_eq!(table.orders().count(), 2);
    }

    #[test]
    fn test_cannot_remove_permanent_order() {
        let ds = MockTableSource::new();
        let mut table = Table::<_, EmptyEntity>::new("test", ds);

        table.add_order(expr!("name"), SortDirection::Ascending);
        let _handle = table.temp_add_order(expr!("age"), SortDirection::Descending);

        // Try to forge a handle to permanent order (negative ID)
        let fake_handle = OrderHandle(-1);
        let result = table.temp_remove_order(fake_handle);
        assert!(result.is_err());
    }
}
