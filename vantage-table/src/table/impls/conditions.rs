use vantage_core::{Result, error};
use vantage_types::Entity;

use crate::{conditions::ConditionHandle, table::Table, traits::table_source::TableSource};

impl<T: TableSource, E: Entity<T::Value>> Table<T, E> {
    /// Add a permanent condition. A literal `column = value` condition (see
    /// [`TableSource::condition_equality`]) also becomes a set invariant.
    pub fn add_condition(&mut self, condition: impl Into<T::Condition>) {
        let condition = condition.into();
        if let Some((column, value)) = self.data_source.condition_equality(&condition) {
            self.add_invariant(column, value);
        }
        let id = -self.next_condition_id;
        self.next_condition_id += 1;
        self.conditions.insert(id, condition);
    }

    /// Add a temporary condition that can be removed later
    pub fn temp_add_condition(&mut self, condition: impl Into<T::Condition>) -> ConditionHandle {
        let id = self.next_condition_id;
        self.next_condition_id += 1;
        self.conditions.insert(id, condition.into());
        ConditionHandle::new(id)
    }

    /// Remove a temporary condition by its handle. A handle this table holds
    /// no condition for (already removed, or from another table) is
    /// not-found.
    pub fn temp_remove_condition(&mut self, handle: ConditionHandle) -> Result<()> {
        if handle.0 <= 0 {
            return Err(error!("Cannot remove permanent condition"));
        }
        match self.conditions.shift_remove(&handle.0) {
            Some(_) => Ok(()),
            None => Err(
                error!("No temporary condition for this handle", handle = handle.0)
                    .mark_not_found(),
            ),
        }
    }

    /// Swap the temporary condition `old` for `new`: `old` is removed when
    /// given, `new` added when given, and the handle of `new` returned. A
    /// stale `old` fails as [`temp_remove_condition`](Self::temp_remove_condition)
    /// does, before `new` is added.
    pub fn replace_temp_condition(
        &mut self,
        old: Option<ConditionHandle>,
        new: Option<T::Condition>,
    ) -> Result<Option<ConditionHandle>> {
        if let Some(handle) = old {
            self.temp_remove_condition(handle)?;
        }
        Ok(new.map(|condition| self.temp_add_condition(condition)))
    }

    /// Get all conditions
    pub fn conditions(&self) -> impl Iterator<Item = &T::Condition> {
        self.conditions.values()
    }

    /// Add a condition using the builder pattern
    pub fn with_condition(mut self, condition: impl Into<T::Condition>) -> Self {
        self.add_condition(condition);
        self
    }

    /// Add a search condition that matches `value` across
    /// [`search_columns`](Self::search_columns).
    pub fn add_search(&mut self, value: &str)
    where
        T: Sized,
    {
        let condition = self.data_source().search_table_condition(self, value);
        self.add_condition(condition);
    }

    /// Add a search condition using the builder pattern.
    pub fn with_search(mut self, value: &str) -> Self
    where
        T: Sized,
    {
        self.add_search(value);
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::mocks::mock_table_source::MockTableSource;

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
    fn literal_equality_condition_registers_an_invariant() {
        use serde_json::json;
        use vantage_expressions::{Expression, ExpressiveEnum};
        let mut table = Table::<_, EmptyEntity>::new("t", MockTableSource::new());
        table.add_condition(Expression::new(
            "{} = {}",
            vec![
                ExpressiveEnum::Nested(Expression::new("parent", vec![])),
                ExpressiveEnum::Nested(Expression::new(
                    "{}",
                    vec![ExpressiveEnum::Scalar(json!("p1"))],
                )),
            ],
        ));
        assert_eq!(table.invariants().get("parent"), Some(&json!("p1")));
        let pattern = "%x%".to_string();
        let _ = table.temp_add_condition(expr_any!("name LIKE {}", pattern));
        assert_eq!(
            table.invariants().len(),
            1,
            "temp conditions register nothing"
        );
    }

    #[test]
    fn removing_a_stale_handle_is_not_found() {
        let mut table = Table::<_, EmptyEntity>::new("test", MockTableSource::new());
        let handle = table.temp_add_condition(expr_any!("temp"));
        table.temp_remove_condition(handle).unwrap();

        let err = table.temp_remove_condition(handle).unwrap_err();
        assert!(err.is_not_found(), "{err}");
        assert_eq!(table.conditions().count(), 0);
    }

    #[test]
    fn replace_temp_condition_swaps_and_clears() {
        let mut table = Table::<_, EmptyEntity>::new("test", MockTableSource::new());
        table.add_condition(expr_any!("perm"));
        let first = table
            .replace_temp_condition(None, Some(expr_any!("a")))
            .unwrap();
        let second = table
            .replace_temp_condition(first, Some(expr_any!("b")))
            .unwrap();
        let previews: Vec<String> = table.conditions().map(|c| c.preview()).collect();
        assert_eq!(previews, ["perm", "b"]);

        let cleared = table.replace_temp_condition(second, None).unwrap();
        assert!(cleared.is_none());
        let previews: Vec<String> = table.conditions().map(|c| c.preview()).collect();
        assert_eq!(previews, ["perm"]);

        let err = table
            .replace_temp_condition(first, Some(expr_any!("c")))
            .unwrap_err();
        assert!(err.is_not_found(), "{err}");
        assert_eq!(table.conditions().count(), 1, "a refused swap adds nothing");
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
