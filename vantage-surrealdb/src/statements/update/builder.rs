use crate::identifier::Identifier;
use crate::types::{AnySurrealType, SurrealType};
use vantage_expressions::Expressive;

use super::{SurrealUpdate, UpdateMode};

impl SurrealUpdate {
    /// Create an UPDATE targeting a whole table by name: `UPDATE tablename ...`
    ///
    /// Useful for bulk updates with `.with_condition()`.
    pub fn table(table: &str) -> Self {
        Self {
            target: Identifier::new(table).expr(),
            mode: UpdateMode::Set,
            fields: indexmap::IndexMap::new(),
            conditions: Vec::new(),
        }
    }

    /// Create an UPDATE with an arbitrary target expression (Thing, table:id, etc).
    pub fn new(target: impl Expressive<AnySurrealType>) -> Self {
        Self {
            target: target.expr(),
            mode: UpdateMode::Set,
            fields: indexmap::IndexMap::new(),
            conditions: Vec::new(),
        }
    }

    /// Switch to CONTENT mode: replaces all fields on the target record.
    pub fn content(mut self) -> Self {
        self.mode = UpdateMode::Content;
        self
    }

    /// Switch to MERGE mode: partial update, keeps fields not mentioned.
    pub fn merge(mut self) -> Self {
        self.mode = UpdateMode::Merge;
        self
    }

    /// Switch to SET mode (the default): `UPDATE target SET key = val, ...`
    pub fn set(mut self) -> Self {
        self.mode = UpdateMode::Set;
        self
    }

    /// Add a typed field. The value is converted to [`AnySurrealType`] via [`SurrealType`].
    pub fn with_field<K: Into<String>, T: SurrealType + 'static>(
        mut self,
        key: K,
        value: T,
    ) -> Self {
        self.fields.insert(key.into(), AnySurrealType::new(value));
        self
    }

    /// Add a pre-built [`AnySurrealType`] field.
    pub fn with_any_field<K: Into<String>>(mut self, key: K, value: AnySurrealType) -> Self {
        self.fields.insert(key.into(), value);
        self
    }

    /// Bulk-load fields from a Record<AnySurrealType>.
    pub fn with_record(mut self, record: &vantage_types::Record<AnySurrealType>) -> Self {
        for (k, v) in record.iter() {
            self.fields.insert(k.clone(), v.clone());
        }
        self
    }

    /// Add a WHERE condition. Multiple conditions are combined with AND.
    pub fn with_condition(mut self, condition: impl Expressive<AnySurrealType>) -> Self {
        self.conditions.push(condition.expr());
        self
    }
}
