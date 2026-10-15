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
