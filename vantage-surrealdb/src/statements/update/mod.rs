//! SurrealDB `UPDATE` statement builder.
//!
//! Builds parameterized `UPDATE` expressions in three modes:
//!
//! - **SET** (default) — `UPDATE target SET key = val, ...`
//! - **CONTENT** — `UPDATE target CONTENT {...}` (replaces all fields)
//! - **MERGE** — `UPDATE target MERGE {...}` (partial update, keeps unmentioned fields)
//!
//! Supports optional `WHERE` conditions for bulk updates.
//!
//! # Examples
//!
//! ```rust,ignore
//! use vantage_surrealdb::{SurrealUpdate, thing::Thing};
//!
//! // SET mode (default) — update specific fields
//! let upd = SurrealUpdate::new(Thing::new("users", "alice"))
//!     .with_field("score", 99i64);
//!
//! // CONTENT mode — replace all fields
//! let upd = SurrealUpdate::new(Thing::new("users", "alice"))
//!     .content()
//!     .with_field("name", "Alice".to_string());
//!
//! // MERGE mode — partial update
//! let upd = SurrealUpdate::new(Thing::new("users", "alice"))
//!     .merge()
//!     .with_field("verified", true);
//!
//! // Bulk update with WHERE
//! let upd = SurrealUpdate::table("users")
//!     .with_field("active", false)
//!     .with_condition(surreal_expr!("last_login < {}", "2020-01-01"));
//!
//! // Execute
//! db.execute(&upd.expr()).await?;
//! ```

use indexmap::IndexMap;
use vantage_expressions::Expressive;

use crate::Expr;
use crate::identifier::Identifier;
use crate::types::{AnySurrealType, SurrealType};

/// Update mode determines the SurrealDB update strategy.
#[derive(Debug, Clone)]
pub enum UpdateMode {
    /// `UPDATE target SET key = val, ...` — set specific fields
    Set,
    /// `UPDATE target CONTENT {...}` — replace all fields
    Content,
    /// `UPDATE target MERGE {...}` — partial update, keeps unmentioned fields
    Merge,
}

/// Builder for SurrealDB `UPDATE` statements.
///
/// Produces `UPDATE target SET/CONTENT/MERGE ... [WHERE ...]`.
/// All field values are passed as parameterized CBOR values, not inlined strings.
pub struct SurrealUpdate {
    /// Target expression (table name, `Thing`, or arbitrary expression).
    pub target: Expr,
    /// Update strategy: SET, CONTENT, or MERGE.
    pub mode: UpdateMode,
    /// Field key-value pairs in insertion order.
    pub fields: IndexMap<String, AnySurrealType>,
    /// Optional WHERE conditions (combined with AND).
    pub conditions: Vec<Expr>,
}

impl SurrealUpdate {
    /// Create an UPDATE targeting a whole table by name: `UPDATE tablename ...`
    ///
    /// Useful for bulk updates with `.with_condition()`.
    pub fn table(table: &str) -> Self {
        Self {
            target: Identifier::new(table).expr(),
            mode: UpdateMode::Set,
            fields: IndexMap::new(),
            conditions: Vec::new(),
        }
    }

    /// Create an UPDATE with an arbitrary target expression (Thing, table:id, etc).
    pub fn new(target: impl Expressive<AnySurrealType>) -> Self {
        Self {
            target: target.expr(),
            mode: UpdateMode::Set,
            fields: IndexMap::new(),
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

    /// Render the statement as a string (for debugging — never use in queries).
    pub fn preview(&self) -> String {
        self.expr().preview()
    }

    /// Build a CBOR object value from current fields (for CONTENT/MERGE).
    fn fields_as_object(&self) -> AnySurrealType {
        let map: Vec<(ciborium::Value, ciborium::Value)> = self
            .fields
            .iter()
            .map(|(k, v)| (ciborium::Value::Text(k.clone()), v.value().clone()))
            .collect();
        AnySurrealType::from_cbor(&ciborium::Value::Map(map))
            .unwrap_or_else(|| AnySurrealType::new(IndexMap::<String, AnySurrealType>::new()))
    }

    fn render_where(&self) -> Option<Expr> {
        if self.conditions.is_empty() {
            return None;
        }
        Some(
            self.conditions
                .iter()
                .cloned()
                .reduce(|a, b| crate::surreal_expr!("{} AND {}", (a), (b)))
                .unwrap(),
        )
    }

    fn append_where(&self, base: Expr) -> Expr {
        match self.render_where() {
            Some(cond) => crate::surreal_expr!("{} WHERE {}", (base), (cond)),
            None => base,
        }
    }
}

impl Expressive<AnySurrealType> for SurrealUpdate {
    fn expr(&self) -> Expr {
        let raw = match self.mode {
            UpdateMode::Set => {
                if self.fields.is_empty() {
                    crate::surreal_expr!("UPDATE {}", (self.target))
                } else {
                    let placeholders: Vec<String> = self
                        .fields
                        .keys()
                        .map(|k| format!("{} = {{}}", Identifier::new(k).expr().preview()))
                        .collect();
                    let template = format!("UPDATE {{}} SET {}", placeholders.join(", "));

                    let mut params: Vec<vantage_expressions::ExpressiveEnum<AnySurrealType>> =
                        vec![vantage_expressions::ExpressiveEnum::Nested(
                            self.target.clone(),
                        )];

                    for value in self.fields.values() {
                        params.push(vantage_expressions::ExpressiveEnum::Scalar(value.clone()));
                    }

                    vantage_expressions::Expression::new(template, params)
                }
            }
            UpdateMode::Content => {
                let obj = self.fields_as_object();
                crate::surreal_expr!("UPDATE {} CONTENT {}", (self.target), obj)
            }
            UpdateMode::Merge => {
                let obj = self.fields_as_object();
                crate::surreal_expr!("UPDATE {} MERGE {}", (self.target), obj)
            }
        };
        self.append_where(raw)
    }
}

impl From<SurrealUpdate> for Expr {
    fn from(update: SurrealUpdate) -> Self {
        update.expr()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thing::Thing;

    #[test]
    fn test_update_set_basic() {
        let update = SurrealUpdate::new(Thing::new("users", "john"))
            .with_field("name", "John".to_string())
            .with_field("age", 30i64);

        let rendered = update.preview();
        assert!(rendered.starts_with("UPDATE users:john SET"));
        assert!(rendered.contains("name = \"John\""));
        assert!(rendered.contains("age = 30"));
    }

    #[test]
    fn test_update_set_empty() {
        let update = SurrealUpdate::new(Thing::new("users", "john"));
        assert_eq!(update.preview(), "UPDATE users:john");
    }

    #[test]
    fn test_update_content() {
        let update = SurrealUpdate::new(Thing::new("users", "john"))
            .content()
            .with_field("name", "Replaced".to_string())
            .with_field("score", 99i64);

        let rendered = update.preview();
        assert!(rendered.starts_with("UPDATE users:john CONTENT"));
    }

    #[test]
    fn test_update_merge() {
        let update = SurrealUpdate::new(Thing::new("users", "john"))
            .merge()
            .with_field("score", 75i64);

        let rendered = update.preview();
        assert!(rendered.starts_with("UPDATE users:john MERGE"));
    }

    #[test]
    fn test_with_any_field() {
        let val = AnySurrealType::new(42i64);
        let update = SurrealUpdate::new(Thing::new("data", "x")).with_any_field("count", val);
        let rendered = update.preview();
        assert!(rendered.contains("count = 42"));
    }

    #[test]
    fn test_with_record() {
        let mut record = vantage_types::Record::new();
        record.insert("a".to_string(), AnySurrealType::new(1i64));
        record.insert("b".to_string(), AnySurrealType::new("hi".to_string()));

        let update = SurrealUpdate::new(Thing::new("t", "1")).with_record(&record);
        let rendered = update.preview();
        assert!(rendered.contains("a = 1"));
        assert!(rendered.contains("b = \"hi\""));
    }

    #[test]
    fn test_update_identifier_escaping() {
        let update = SurrealUpdate::new(crate::surreal_expr!("⟨SELECT⟩:test"))
            .with_field("FROM", "value".to_string());

        let rendered = update.preview();
        assert!(rendered.contains("⟨FROM⟩ = \"value\""));
    }

    #[test]
    fn test_update_produces_parameterized_expression() {
        let update = SurrealUpdate::new(Thing::new("t", "1"))
            .with_field("x", 10i64)
            .with_field("y", 20i64);

        let expr = update.expr();
        assert!(expr.template.contains("{}"));
        assert_eq!(expr.parameters.len(), 3); // target + 2 fields
    }

    #[test]
    fn test_update_with_thing_field() {
        let update = SurrealUpdate::new(Thing::new("order", "o1"))
            .with_field("customer", Thing::new("user", "alice"));

        let rendered = update.preview();
        assert!(rendered.contains("UPDATE order:o1 SET"));
        assert!(rendered.contains("customer ="));
    }

    #[test]
    fn test_mode_switching() {
        let update = SurrealUpdate::new(Thing::new("t", "1"))
            .content()
            .with_field("a", 1i64);
        assert!(update.preview().contains("CONTENT"));

        let update = update.merge();
        assert!(update.preview().contains("MERGE"));

        let update = update.set();
        assert!(update.preview().contains("SET"));
    }

    #[test]
    fn test_with_condition() {
        let update = SurrealUpdate::table("users")
            .with_field("active", false)
            .with_condition(crate::surreal_expr!("last_login < {}", "2020-01-01"));

        let p = update.preview();
        assert!(p.contains("UPDATE users SET"));
        assert!(p.contains("active = false"));
        assert!(p.contains("WHERE last_login < \"2020-01-01\""));
    }

    #[test]
    fn test_with_multiple_conditions() {
        let update = SurrealUpdate::table("logs")
            .with_field("archived", true)
            .with_condition(crate::surreal_expr!("level = {}", "debug"))
            .with_condition(crate::surreal_expr!("age > {}", 30i64));

        assert_eq!(
            update.preview(),
            "UPDATE logs SET archived = true WHERE level = \"debug\" AND age > 30"
        );
    }

    #[test]
    fn test_table_constructor() {
        let update = SurrealUpdate::table("products").with_field("in_stock", true);
        let p = update.preview();
        assert!(p.starts_with("UPDATE products SET"));
    }

    #[test]
    fn test_with_arbitrary_target_expression() {
        let upd = SurrealUpdate::new(crate::surreal_expr!("user WHERE active = true"))
            .with_field("checked", true);

        let p = upd.preview();
        assert!(p.contains("UPDATE user WHERE active = true SET"));
    }

    #[test]
    fn test_content_with_condition() {
        let update = SurrealUpdate::table("cache")
            .content()
            .with_field("data", "refreshed".to_string())
            .with_condition(crate::surreal_expr!("expired = {}", true));

        let p = update.preview();
        assert!(p.contains("CONTENT"));
        assert!(p.contains("WHERE expired = true"));
    }

    #[test]
    fn test_merge_with_condition() {
        let update = SurrealUpdate::table("users")
            .merge()
            .with_field("verified", true)
            .with_condition(crate::surreal_expr!("email_confirmed = {}", true));

        let p = update.preview();
        assert!(p.contains("MERGE"));
        assert!(p.contains("WHERE email_confirmed = true"));
    }
}
