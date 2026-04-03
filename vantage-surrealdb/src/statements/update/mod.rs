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

pub struct SurrealUpdate {
    pub target: Expr,
    pub mode: UpdateMode,
    pub fields: IndexMap<String, AnySurrealType>,
}

impl SurrealUpdate {
    /// Create an UPDATE with a table:id target expression.
    pub fn new(target: impl Expressive<AnySurrealType>) -> Self {
        Self {
            target: target.expr(),
            mode: UpdateMode::Set,
            fields: IndexMap::new(),
        }
    }

    pub fn content(mut self) -> Self {
        self.mode = UpdateMode::Content;
        self
    }

    pub fn merge(mut self) -> Self {
        self.mode = UpdateMode::Merge;
        self
    }

    pub fn set(mut self) -> Self {
        self.mode = UpdateMode::Set;
        self
    }

    pub fn set_field<K: Into<String>, T: SurrealType + 'static>(
        mut self,
        key: K,
        value: T,
    ) -> Self {
        self.fields.insert(key.into(), AnySurrealType::new(value));
        self
    }

    pub fn set_any_field<K: Into<String>>(mut self, key: K, value: AnySurrealType) -> Self {
        self.fields.insert(key.into(), value);
        self
    }

    /// Bulk-load fields from a Record<AnySurrealType>.
    pub fn set_record(mut self, record: &vantage_types::Record<AnySurrealType>) -> Self {
        for (k, v) in record.iter() {
            self.fields.insert(k.clone(), v.clone());
        }
        self
    }

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
}

impl Expressive<AnySurrealType> for SurrealUpdate {
    fn expr(&self) -> Expr {
        match self.mode {
            UpdateMode::Set => {
                if self.fields.is_empty() {
                    return crate::surreal_expr!("UPDATE {}", (self.target));
                }

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
            UpdateMode::Content => {
                let obj = self.fields_as_object();
                crate::surreal_expr!("UPDATE {} CONTENT {}", (self.target), obj)
            }
            UpdateMode::Merge => {
                let obj = self.fields_as_object();
                crate::surreal_expr!("UPDATE {} MERGE {}", (self.target), obj)
            }
        }
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
            .set_field("name", "John".to_string())
            .set_field("age", 30i64);

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
            .set_field("name", "Replaced".to_string())
            .set_field("score", 99i64);

        let rendered = update.preview();
        assert!(rendered.starts_with("UPDATE users:john CONTENT"));
    }

    #[test]
    fn test_update_merge() {
        let update = SurrealUpdate::new(Thing::new("users", "john"))
            .merge()
            .set_field("score", 75i64);

        let rendered = update.preview();
        assert!(rendered.starts_with("UPDATE users:john MERGE"));
    }

    #[test]
    fn test_update_set_any_field() {
        let val = AnySurrealType::new(42i64);
        let update = SurrealUpdate::new(Thing::new("data", "x")).set_any_field("count", val);
        let rendered = update.preview();
        assert!(rendered.contains("count = 42"));
    }

    #[test]
    fn test_update_set_record() {
        let mut record = vantage_types::Record::new();
        record.insert("a".to_string(), AnySurrealType::new(1i64));
        record.insert("b".to_string(), AnySurrealType::new("hi".to_string()));

        let update = SurrealUpdate::new(Thing::new("t", "1")).set_record(&record);
        let rendered = update.preview();
        assert!(rendered.contains("a = 1"));
        assert!(rendered.contains("b = \"hi\""));
    }

    #[test]
    fn test_update_identifier_escaping() {
        let update = SurrealUpdate::new(crate::surreal_expr!("⟨SELECT⟩:test"))
            .set_field("FROM", "value".to_string());

        let rendered = update.preview();
        assert!(rendered.contains("⟨FROM⟩ = \"value\""));
    }

    #[test]
    fn test_update_produces_parameterized_expression() {
        let update = SurrealUpdate::new(Thing::new("t", "1"))
            .set_field("x", 10i64)
            .set_field("y", 20i64);

        let expr = update.expr();
        assert!(expr.template.contains("{}"));
        assert_eq!(expr.parameters.len(), 3); // target + 2 fields
    }

    #[test]
    fn test_update_with_thing_field() {
        let update = SurrealUpdate::new(Thing::new("order", "o1"))
            .set_field("customer", Thing::new("user", "alice"));

        let rendered = update.preview();
        assert!(rendered.contains("UPDATE order:o1 SET"));
        assert!(rendered.contains("customer ="));
    }

    #[test]
    fn test_mode_switching() {
        let update = SurrealUpdate::new(Thing::new("t", "1"))
            .content()
            .set_field("a", 1i64);
        assert!(update.preview().contains("CONTENT"));

        let update = update.merge();
        assert!(update.preview().contains("MERGE"));

        let update = update.set();
        assert!(update.preview().contains("SET"));
    }
}
