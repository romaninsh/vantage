use vantage_expressions::Expressive;

use crate::mysql::types::AnyMysqlType;

use super::MysqlDelete;

impl MysqlDelete {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            conditions: Vec::new(),
        }
    }

    pub fn with_condition(mut self, condition: impl Expressive<AnyMysqlType>) -> Self {
        self.conditions.push(condition.expr());
        self
    }
}
