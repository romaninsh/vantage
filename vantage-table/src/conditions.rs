use vantage_expressions::{DataSource, OwnedExpression};

use super::{Entity, Table};

impl<T: DataSource<OwnedExpression>, E: Entity> Table<T, E> {
    /// Add a condition to limit what records the table represents
    pub fn add_condition(&mut self, condition: OwnedExpression) {
        self.conditions.push(condition);
    }

    /// Get all conditions
    pub fn conditions(&self) -> &[OwnedExpression] {
        &self.conditions
    }
}
