use vantage_core::{Result, error};
use vantage_expressions::Expression;
use vantage_types::Entity;

use crate::{conditions::ConditionHandle, table::Table, traits::table_source::TableSource};

impl<T: TableSource, E: Entity> Table<T, E> {
    /// Add a permanent condition to limit what records the table represents
    pub fn add_condition(&mut self, condition: Expression<T::Value>) {
        let id = -self.next_condition_id;
        self.next_condition_id += 1;
        self.conditions.insert(id, condition);
    }

    /// Add a temporary condition that can be removed later
    pub fn temp_add_condition(&mut self, condition: Expression<T::Value>) -> ConditionHandle {
        let id = self.next_condition_id;
        self.next_condition_id += 1;
        self.conditions.insert(id, condition);
        ConditionHandle::new(id)
    }

    /// Remove a temporary condition by its handle
    pub fn temp_remove_condition(&mut self, handle: ConditionHandle) -> Result<()> {
        if handle.0 <= 0 {
            return Err(error!("Cannot remove permanent condition"));
        }
        self.conditions.shift_remove(&handle.0);
        Ok(())
    }

    /// Get all conditions
    pub fn conditions(&self) -> impl Iterator<Item = &Expression<T::Value>> {
        self.conditions.values()
    }

    /// Add a condition using the builder pattern
    pub fn with_condition(mut self, condition: Expression<T::Value>) -> Self {
        self.add_condition(condition);
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::mocks::tablesource::MockTableSource;

    use super::*;
    use vantage_expressions::expr_any;
    use vantage_types::EmptyEntity;

    #[test]
    fn test_temp_conditions() {
        let ds = MockTableSource::new();
        let mut table = Table::<_, EmptyEntity>::new("test", ds);

        // Add permanent condition
        table.add_condition(expr_any!("perm1"));
        assert_eq!(table.conditions().count(), 1);

        // Add temp conditions
        let handle1 = table.temp_add_condition(expr_any!("temp1"));
        let handle2 = table.temp_add_condition(expr_any!("temp2"));
        assert_eq!(table.conditions().count(), 3);

        // Remove one temp condition
        table.temp_remove_condition(handle1).unwrap();
        assert_eq!(table.conditions().count(), 2);

        // Add another permanent
        table.add_condition(expr_any!("perm2"));
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

        table.add_condition(expr_any!("perm"));
        let _handle = table.temp_add_condition(expr_any!("temp"));

        // Try to forge a handle to permanent condition (negative ID)
        let fake_handle = ConditionHandle::new(-1);
        let result = table.temp_remove_condition(fake_handle);
        assert!(result.is_err());
    }
}
