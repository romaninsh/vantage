use super::{Entity, Table, TableSource};
use vantage_core::{Result, error};

/// Handle for temporary conditions that can be removed
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConditionHandle(i64);

impl<T: TableSource, E: Entity> Table<T, E> {
    /// Add a permanent condition to limit what records the table represents
    pub fn add_condition(&mut self, condition: T::Expr) {
        let id = -self.next_condition_id;
        self.next_condition_id += 1;
        self.conditions.insert(id, condition);
    }

    /// Add a temporary condition that can be removed later
    pub fn temp_add_condition(&mut self, condition: T::Expr) -> ConditionHandle {
        let id = self.next_condition_id;
        self.next_condition_id += 1;
        self.conditions.insert(id, condition);
        ConditionHandle(id)
    }

    /// Remove a temporary condition by its handle
    ///
    /// # Errors
    ///
    /// Returns error if the handle refers to a permanent condition (added via `add_condition`)
    pub fn temp_remove_condition(&mut self, handle: ConditionHandle) -> Result<()> {
        if handle.0 <= 0 {
            return Err(error!("Cannot remove permanent condition").into());
        }
        self.conditions.shift_remove(&handle.0);
        Ok(())
    }

    /// Get all conditions
    pub fn conditions(&self) -> impl Iterator<Item = &T::Expr> {
        self.conditions.values()
    }

    /// Add a condition using the builder pattern
    pub fn with_condition(mut self, condition: T::Expr) -> Self {
        self.add_condition(condition);
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
    fn test_temp_conditions() {
        let ds = MockTableSource::new();
        let mut table = Table::<_, EmptyEntity>::new("test", ds);

        // Add permanent condition
        table.add_condition(expr!("perm1"));
        assert_eq!(table.conditions().count(), 1);

        // Add temp conditions
        let handle1 = table.temp_add_condition(expr!("temp1"));
        let handle2 = table.temp_add_condition(expr!("temp2"));
        assert_eq!(table.conditions().count(), 3);

        // Remove one temp condition
        table.temp_remove_condition(handle1).unwrap();
        assert_eq!(table.conditions().count(), 2);

        // Add another permanent
        table.add_condition(expr!("perm2"));
        assert_eq!(table.conditions().count(), 3);

        // Remove second temp
        table.temp_remove_condition(handle2).unwrap();
        assert_eq!(table.conditions().count(), 2);

        // Verify we have exactly 2 conditions left (both permanent)
        assert_eq!(table.conditions().count(), 2);
    }

    #[test]
    fn test_cannot_remove_permanent_condition() {
        let ds = MockTableSource::new();
        let mut table = Table::<_, EmptyEntity>::new("test", ds);

        table.add_condition(expr!("perm"));
        let _handle = table.temp_add_condition(expr!("temp"));

        // Try to forge a handle to permanent condition (negative ID)
        let fake_handle = ConditionHandle(-1);
        let result = table.temp_remove_condition(fake_handle);
        assert!(result.is_err());
    }
}
