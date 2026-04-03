use crate::identifier::Identifier;
use crate::types::AnySurrealType;
use vantage_expressions::Expressive;

use super::SurrealDelete;

impl SurrealDelete {
    /// Delete all records from a table: `DELETE tablename`
    pub fn table(table: &str) -> Self {
        Self {
            target: Identifier::new(table).expr(),
            conditions: Vec::new(),
        }
    }

    /// Delete a specific target (e.g. a [`Thing`](crate::thing::Thing) record ID).
    pub fn new(target: impl Expressive<AnySurrealType>) -> Self {
        Self {
            target: target.expr(),
            conditions: Vec::new(),
        }
    }

    /// Add a WHERE condition. Multiple conditions are combined with AND.
    pub fn with_condition(mut self, condition: impl Expressive<AnySurrealType>) -> Self {
        self.conditions.push(condition.expr());
        self
    }
}
