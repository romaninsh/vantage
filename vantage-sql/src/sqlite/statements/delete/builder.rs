use serde_json::Value as JsonValue;
use vantage_expressions::Expressive;

use super::SqliteDelete;

impl SqliteDelete {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            conditions: Vec::new(),
        }
    }

    pub fn with_condition(mut self, condition: impl Expressive<JsonValue>) -> Self {
        self.conditions.push(condition.expr());
        self
    }
}
