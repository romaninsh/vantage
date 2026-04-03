use indexmap::IndexMap;
use vantage_expressions::Expressive;

use crate::Expr;
use crate::identifier::Identifier;
use crate::types::{AnySurrealType, SurrealType};

pub struct SurrealInsert {
    pub table: Identifier,
    pub id: Option<Identifier>,
    pub fields: IndexMap<String, AnySurrealType>,
}

impl SurrealInsert {
    pub fn new(table: &str) -> Self {
        Self {
            table: Identifier::new(table),
            id: None,
            fields: IndexMap::new(),
        }
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(Identifier::new(id.into()));
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

    fn target_expr(&self) -> Expr {
        match &self.id {
            Some(id) => crate::surreal_expr!("{}:{}", (self.table), (id)),
            None => self.table.expr(),
        }
    }

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
            .set_field("name", "John".to_string())
            .set_field("age", 30i64);

        let rendered = insert.preview();
        assert!(rendered.starts_with("CREATE users SET"));
        assert!(rendered.contains("name = \"John\""));
        assert!(rendered.contains("age = 30"));
    }

    #[test]
    fn test_insert_with_id() {
        let insert = SurrealInsert::new("users")
            .with_id("john")
            .set_field("name", "John".to_string());

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
        let insert = SurrealInsert::new("SELECT").set_field("FROM", "value".to_string());

        let rendered = insert.preview();
        assert!(rendered.contains("CREATE ⟨SELECT⟩"));
        assert!(rendered.contains("⟨FROM⟩ = \"value\""));
    }

    #[test]
    fn test_insert_produces_parameterized_expression() {
        let insert = SurrealInsert::new("users")
            .set_field("name", "Alice".to_string())
            .set_field("age", 25i64);

        let expr = insert.expr();
        // Template should have {} placeholders (not inlined values)
        assert!(expr.template.contains("{}"));
        // Parameters should contain the scalar values
        assert_eq!(expr.parameters.len(), 3); // target + 2 fields
    }

    #[test]
    fn test_set_any_field() {
        let val = AnySurrealType::new(42i64);
        let insert = SurrealInsert::new("data").set_any_field("count", val);
        let rendered = insert.preview();
        assert!(rendered.contains("count = 42"));
    }

    #[test]
    fn test_thing_field() {
        use crate::thing::Thing;
        let insert = SurrealInsert::new("order").set_field("customer", Thing::new("user", "alice"));

        let rendered = insert.preview();
        assert!(rendered.contains("CREATE order SET"));
        // Thing renders via its Expressive impl when previewed through AnySurrealType Display
    }
}
