//! Sorting-related support data-types

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
pub struct OrderHandle(pub(super) i64);

#[cfg(test)]
mod tests {
    use crate::{
        mocks::tablesource::MockTableSource,
        table::{Table, sorting::OrderByExt},
    };

    use super::*;
    use vantage_core::EmptyEntity;
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
