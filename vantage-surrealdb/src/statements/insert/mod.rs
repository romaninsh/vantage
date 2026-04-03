//! SurrealDB `CREATE` statement builder.
//!
//! Builds parameterized `CREATE table SET ...` or `CREATE table:id SET ...`
//! expressions for execution via [`ExprDataSource::execute()`].
//!
//! # Examples
//!
//! ```rust,ignore
//! use vantage_surrealdb::{SurrealInsert, thing::Thing};
//!
//! // Auto-generated ID
//! let ins = SurrealInsert::new("users")
//!     .with_field("name", "Alice".to_string())
//!     .with_field("age", 30i64);
//!
//! // Explicit ID
//! let ins = SurrealInsert::new("users")
//!     .with_id("alice")
//!     .with_field("name", "Alice".to_string());
//!
//! // Thing reference field
//! let ins = SurrealInsert::new("order")
//!     .with_id("o1")
//!     .with_field("customer", Thing::new("user", "alice"));
//!
//! // Execute
//! db.execute(&ins.expr()).await?;
//! ```

use indexmap::IndexMap;
use vantage_expressions::Expressive;

use crate::Expr;
use crate::identifier::Identifier;
use crate::types::{AnySurrealType, SurrealType};

/// Builder for SurrealDB `CREATE` statements.
///
/// Produces `CREATE table SET key = val, ...` or `CREATE table:id SET ...`.
/// All field values are passed as parameterized CBOR values, not inlined strings.
pub struct SurrealInsert {
    /// Target table (auto-escaped if reserved keyword).
    pub table: Identifier,
    /// Optional record ID. When set, produces `CREATE table:id ...`.
    pub id: Option<Identifier>,
    /// Field key-value pairs in insertion order.
    pub fields: IndexMap<String, AnySurrealType>,
}

impl SurrealInsert {
    /// Create a new insert targeting the given table.
    pub fn new(table: &str) -> Self {
        Self {
            table: Identifier::new(table),
            id: None,
            fields: IndexMap::new(),
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

    fn target_expr(&self) -> Expr {
        match &self.id {
            Some(id) => crate::surreal_expr!("{}:{}", (self.table), (id)),
            None => self.table.expr(),
        }
    }

    /// Render the statement as a string (for debugging — never use in queries).
    pub fn preview(&self) -> String {
        self.expr().preview()
    }
}

impl Expressive<AnySurrealType> for SurrealInsert {
    fn expr(&self) -> Expr {
        let target = self.target_expr();

        if self.fields.is_empty() {
            return crate::surreal_expr!("CREATE {}", (target));
        }

        // Build "key1 = {}, key2 = {}" with field values as scalar params
        let keys: Vec<&String> = self.fields.keys().collect();
        let placeholders: Vec<String> = keys
            .iter()
            .map(|k| format!("{} = {{}}", Identifier::new(*k).expr().preview()))
            .collect();
        let template = format!("CREATE {{}} SET {}", placeholders.join(", "));

        let mut params: Vec<vantage_expressions::ExpressiveEnum<AnySurrealType>> =
            vec![vantage_expressions::ExpressiveEnum::Nested(target)];

        for value in self.fields.values() {
            params.push(vantage_expressions::ExpressiveEnum::Scalar(value.clone()));
        }

        vantage_expressions::Expression::new(template, params)
    }
}

impl From<SurrealInsert> for Expr {
    fn from(insert: SurrealInsert) -> Self {
        insert.expr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_insert() {
        let insert = SurrealInsert::new("users")
            .with_field("name", "John".to_string())
            .with_field("age", 30i64);

        let rendered = insert.preview();
        assert!(rendered.starts_with("CREATE users SET"));
        assert!(rendered.contains("name = \"John\""));
        assert!(rendered.contains("age = 30"));
    }

    #[test]
    fn test_insert_with_id() {
        let insert = SurrealInsert::new("users")
            .with_id("john")
            .with_field("name", "John".to_string());

        let rendered = insert.preview();
        assert!(rendered.starts_with("CREATE users:john SET"));
        assert!(rendered.contains("name = \"John\""));
    }

    #[test]
    fn test_empty_insert() {
        let insert = SurrealInsert::new("users");
        assert_eq!(insert.preview(), "CREATE users");
    }

    #[test]
    fn test_empty_insert_with_id() {
        let insert = SurrealInsert::new("users").with_id("john");
        assert_eq!(insert.preview(), "CREATE users:john");
    }

    #[test]
    fn test_identifier_escaping() {
        let insert = SurrealInsert::new("SELECT").with_field("FROM", "value".to_string());

        let rendered = insert.preview();
        assert!(rendered.contains("CREATE ⟨SELECT⟩"));
        assert!(rendered.contains("⟨FROM⟩ = \"value\""));
    }

    #[test]
    fn test_insert_produces_parameterized_expression() {
        let insert = SurrealInsert::new("users")
            .with_field("name", "Alice".to_string())
            .with_field("age", 25i64);

        let expr = insert.expr();
        assert!(expr.template.contains("{}"));
        assert_eq!(expr.parameters.len(), 3); // target + 2 fields
    }

    #[test]
    fn test_with_any_field() {
        let val = AnySurrealType::new(42i64);
        let insert = SurrealInsert::new("data").with_any_field("count", val);
        let rendered = insert.preview();
        assert!(rendered.contains("count = 42"));
    }

    #[test]
    fn test_with_record() {
        let mut record = vantage_types::Record::new();
        record.insert("a".to_string(), AnySurrealType::new(1i64));
        record.insert("b".to_string(), AnySurrealType::new("hi".to_string()));

        let insert = SurrealInsert::new("t").with_id("1").with_record(&record);
        let p = insert.preview();
        assert!(p.contains("a = 1"));
        assert!(p.contains("b = \"hi\""));
    }

    #[test]
    fn test_thing_field() {
        use crate::thing::Thing;
        let insert =
            SurrealInsert::new("order").with_field("customer", Thing::new("user", "alice"));

        let rendered = insert.preview();
        assert!(rendered.contains("CREATE order SET"));
    }
}
