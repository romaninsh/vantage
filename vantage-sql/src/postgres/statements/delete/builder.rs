use vantage_expressions::Expressive;

use crate::postgres::types::AnyPostgresType;

use super::PostgresDelete;

impl PostgresDelete {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            conditions: Vec::new(),
        }
    }

    pub fn with_condition(mut self, condition: impl Expressive<AnyPostgresType>) -> Self {
        self.conditions.push(condition.expr());
        self
    }
}
