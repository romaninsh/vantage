use indexmap::IndexMap;
use vantage_types::Record;

use crate::sqlite::types::AnySqliteType;

use super::SqliteInsert;

impl SqliteInsert {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            fields: IndexMap::new(),
        }
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<AnySqliteType>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    pub fn with_record(mut self, record: &Record<AnySqliteType>) -> Self {
        for (key, value) in record.iter() {
            self.fields.insert(key.clone(), value.clone());
        }
        self
    }
}
