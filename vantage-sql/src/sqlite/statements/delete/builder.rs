use vantage_expressions::Expressive;

use crate::sqlite::types::AnySqliteType;

use super::SqliteDelete;

impl SqliteDelete {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            conditions: Vec::new(),
        }
    }

    pub fn with_condition(mut self, condition: impl Expressive<AnySqliteType>) -> Self {
        self.conditions.push(condition.expr());
        self
    }
}
