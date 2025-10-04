use super::{Entity, Table, TableSource};

impl<T: TableSource, E: Entity> Table<T, E> {
    /// Add a condition to limit what records the table represents
    pub fn add_condition(&mut self, condition: T::Expr) {
        self.conditions.push(condition);
    }

    /// Get all conditions
    pub fn conditions(&self) -> &[T::Expr] {
        &self.conditions
    }

    /// Add a condition using the builder pattern
    pub fn with_condition(mut self, condition: T::Expr) -> Self {
        self.add_condition(condition);
        self
    }
}
