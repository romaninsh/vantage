use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use vantage_expressions::Expressive;
use vantage_types::Record;

use super::SqliteUpdate;

impl SqliteUpdate {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            fields: IndexMap::new(),
            conditions: Vec::new(),
        }
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<JsonValue>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    pub fn with_record(mut self, record: &Record<JsonValue>) -> Self {
        for (key, value) in record.iter() {
            self.fields.insert(key.clone(), value.clone());
        }
        self
    }

    pub fn with_condition(mut self, condition: impl Expressive<JsonValue>) -> Self {
        self.conditions.push(condition.expr());
        self
    }
}
