use indexmap::IndexMap;
use serde_json::Value as JsonValue;
use vantage_types::Record;

use super::SqliteInsert;

impl SqliteInsert {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            fields: IndexMap::new(),
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
}
