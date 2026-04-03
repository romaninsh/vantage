use crate::Expr;
use crate::identifier::Identifier;
use crate::types::{AnySurrealType, SurrealType};
use vantage_expressions::Expressive;

use super::SurrealInsert;

impl SurrealInsert {
    /// Create a new insert targeting the given table.
    pub fn new(table: &str) -> Self {
        Self {
            table: Identifier::new(table),
            id: None,
            fields: indexmap::IndexMap::new(),
        }
    }

    /// Set an explicit record ID: `CREATE table:id ...`
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(Identifier::new(id.into()));
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

    pub(crate) fn target_expr(&self) -> Expr {
        match &self.id {
            Some(id) => crate::surreal_expr!("{}:{}", (self.table), (id)),
            None => self.table.expr(),
        }
    }
}
