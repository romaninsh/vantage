use indexmap::IndexMap;
use vantage_types::Record;

use crate::postgres::types::AnyPostgresType;

use super::PostgresInsert;

impl PostgresInsert {
    pub fn new(table: &str) -> Self {
        Self {
            table: table.to_string(),
            fields: IndexMap::new(),
        }
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<AnyPostgresType>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    pub fn with_record(mut self, record: &Record<AnyPostgresType>) -> Self {
        for (key, value) in record.iter() {
            self.fields.insert(key.clone(), value.clone());
        }
        self
    }
}
