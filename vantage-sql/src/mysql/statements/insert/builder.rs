use indexmap::IndexMap;
use vantage_types::Record;

use crate::mysql::types::AnyMysqlType;

use super::MysqlInsert;

impl MysqlInsert {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            fields: IndexMap::new(),
        }
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<AnyMysqlType>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    pub fn with_record(mut self, record: &Record<AnyMysqlType>) -> Self {
        for (key, value) in record.iter() {
            self.fields.insert(key.clone(), value.clone());
        }
        self
    }
}
