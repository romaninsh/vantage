use vantage_expressions::{DataSource, Expression};

use super::{Entity, Table};

impl<T: DataSource<Expression>, E: Entity> Table<T, E> {
    /// Add a condition to limit what records the table represents
    pub fn add_condition(&mut self, condition: Expression) {
        self.conditions.push(condition);
    }

    /// Get all conditions
    pub fn conditions(&self) -> &[Expression] {
        &self.conditions
    }
}
