use super::{Entity, Table, TableSource};
use vantage_core::{Result, error};
use vantage_expressions::Expression;

/// Sort direction for ordering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// Order specification combining expression and direction
#[derive(Debug, Clone)]
pub struct OrderBy<E> {
    pub expression: E,
    pub direction: SortDirection,
}

impl<E> OrderBy<E> {
    /// Create a new OrderBy with ascending direction
    pub fn ascending(expression: E) -> Self {
        Self {
            expression,
            direction: SortDirection::Ascending,
        }
    }

    /// Create a new OrderBy with descending direction
    pub fn descending(expression: E) -> Self {
        Self {
            expression,
            direction: SortDirection::Descending,
        }
    }
}

/// Handle for temporary order clauses that can be removed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OrderHandle(i64);

impl<T: TableSource, E: Entity> Table<T, E> {
    /// Add a permanent order clause
    pub fn add_order(&mut self, order: OrderBy<T::Expr>) {
        let id = -self.next_order_id;
        self.next_order_id += 1;
        self.order_by
            .insert(id, (order.expression, order.direction));
    }

    /// Add a temporary order clause that can be removed later
    pub fn temp_add_order(&mut self, order: OrderBy<T::Expr>) -> OrderHandle {
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
    pub fn orders(&self) -> impl Iterator<Item = &(T::Expr, SortDirection)> {
        self.order_by.values()
    }

    /// Add an order clause using the builder pattern
    pub fn with_order(mut self, order: OrderBy<T::Expr>) -> Self {
        self.add_order(order);
        self
    }
}

/// Extension trait for creating OrderBy from expressions and columns
pub trait OrderByExt {
    /// Create an ascending order specification
    fn ascending(&self) -> OrderBy<Expression>;

    /// Create a descending order specification
    fn descending(&self) -> OrderBy<Expression>;
}

// Blanket implementation for anything that implements ColumnLike
impl<T: crate::ColumnLike> OrderByExt for T {
    fn ascending(&self) -> OrderBy<Expression> {
        OrderBy::ascending(self.expr())
    }

    fn descending(&self) -> OrderBy<Expression> {
        OrderBy::descending(self.expr())
    }
}

// Direct implementation for Expression
impl OrderByExt for Expression {
    fn ascending(&self) -> OrderBy<Expression> {
        OrderBy::ascending(self.clone())
    }

    fn descending(&self) -> OrderBy<Expression> {
        OrderBy::descending(self.clone())
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
        table.add_order(OrderBy::ascending(expr!("name")));
        assert_eq!(table.orders().count(), 1);

        // Add temp orders
        let handle1 = table.temp_add_order(OrderBy::descending(expr!("age")));
        let handle2 = table.temp_add_order(OrderBy::ascending(expr!("created_at")));
        assert_eq!(table.orders().count(), 3);

        // Remove one temp order
        table.temp_remove_order(handle1).unwrap();
        assert_eq!(table.orders().count(), 2);

        // Add another permanent
        table.add_order(OrderBy::ascending(expr!("id")));
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

        table.add_order(OrderBy::ascending(expr!("name")));
        let _handle = table.temp_add_order(OrderBy::descending(expr!("age")));

        // Try to forge a handle to permanent order (negative ID)
        let fake_handle = OrderHandle(-1);
        let result = table.temp_remove_order(fake_handle);
        assert!(result.is_err());
    }

    #[test]
    fn test_ergonomic_ordering() {
        use vantage_expressions::expr;

        let ds = MockTableSource::new();
        let mut table = Table::<_, EmptyEntity>::new("test", ds);

        // Test with expr!().ascending()
        table.add_order(expr!("name").ascending());
        assert_eq!(table.orders().count(), 1);

        // Test with expr!().descending()
        table.add_order(expr!("age").descending());
        assert_eq!(table.orders().count(), 2);

        // Verify directions
        let orders: Vec<_> = table.orders().collect();
        assert!(matches!(orders[0].1, SortDirection::Ascending));
        assert!(matches!(orders[1].1, SortDirection::Descending));
    }
}
