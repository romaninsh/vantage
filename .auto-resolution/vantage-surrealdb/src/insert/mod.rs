use crate::expression::{IntoSurrealExpr, SurrealExpr};
use crate::identifier::Identifier;
use crate::operation::Expressive;
use crate::surrealdb::SurrealDB;
use std::collections::HashMap;
use surreal_client::types::{AnySurrealType, SurrealType};
use vantage_core::Result;
use vantage_expressions::Expression;

pub struct SurrealInsert {
    pub table: Identifier,
    pub id: Option<Identifier>,
    pub fields: HashMap<String, AnySurrealType>,
}

impl SurrealInsert {
    pub fn new(table: &str) -> Self {
        Self {
            table: Identifier::new(table),
            id: None,
            fields: HashMap::new(),
        }
    }

    pub fn with_id<T: Into<String>>(mut self, id: T) -> Self {
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

    fn render_target(&self) -> Expression {
        if let Some(id) = &self.id {
            Expression::new(
                format!("{}:{}", self.table.expr().preview(), id.expr().preview()),
                vec![],
            )
        } else {
            self.table.expr()
        }
    }

    pub fn render(&self) -> Expression {
        let target = self.render_target();

        if self.fields.is_empty() {
            return Expression::new(format!("CREATE {}", target.preview()), vec![]);
        }

        let field_assignments: Vec<String> = self
            .fields
            .iter()
            .map(|(key, value)| {
                // Use Display implementation of AnySurrealType which preserves types
                format!("{} = {}", Identifier::new(key).expr().preview(), value)
            })
            .collect();

        Expression::new(
            format!(
                "CREATE {} SET {}",
                target.preview(),
                field_assignments.join(", ")
            ),
            vec![],
        )
    }

    pub fn preview(&self) -> String {
        self.render().preview()
    }

    /// Create a CBOR-native query for execution
    pub fn render_cbor(&self) -> (String, ciborium::value::Value) {
        let target = self.render_target();

        if self.fields.is_empty() {
            return (
                format!("CREATE {}", target.preview()),
                ciborium::value::Value::Null,
            );
        }

        // Build template with placeholders
        let field_placeholders: Vec<String> = self
            .fields
            .keys()
            .map(|key| format!("{} = ${}", Identifier::new(key).expr().preview(), key))
            .collect();

        let query = format!(
            "CREATE {} SET {}",
            target.preview(),
            field_placeholders.join(", ")
        );

        // Build CBOR parameters map
        let mut params = Vec::new();
        for (key, value) in &self.fields {
            params.push((ciborium::value::Value::Text(key.clone()), value.cborify()));
        }

        (query, ciborium::value::Value::Map(params))
    }

    pub async fn execute(&self, ds: &SurrealDB) -> Result<serde_json::Value> {
        // Use CBOR-native query execution
        let (query, params) = self.render_cbor();
        let result = ds.query_cbor(&query, Some(params)).await.map_err(|e| {
            vantage_core::error!(
                "Failed to execute CBOR insert query",
                query = &query,
                error = e.to_string()
            )
        })?;

        // Convert CBOR result back to JSON for compatibility
        fn cbor_to_json(cbor: ciborium::value::Value) -> serde_json::Value {
            match cbor {
                ciborium::value::Value::Null => serde_json::Value::Null,
                ciborium::value::Value::Bool(b) => serde_json::Value::Bool(b),
                ciborium::value::Value::Integer(i) => {
                    serde_json::Number::from(i128::from(i) as i64).into()
                }
                ciborium::value::Value::Float(f) => serde_json::Number::from_f64(f)
                    .unwrap_or_else(|| 0.into())
                    .into(),
                ciborium::value::Value::Text(s) => serde_json::Value::String(s),
                ciborium::value::Value::Array(arr) => {
                    serde_json::Value::Array(arr.into_iter().map(cbor_to_json).collect())
                }
                ciborium::value::Value::Map(map) => {
                    let mut obj = serde_json::Map::new();
                    for (k, v) in map {
                        if let ciborium::value::Value::Text(key) = k {
                            obj.insert(key, cbor_to_json(v));
                        }
                    }
                    serde_json::Value::Object(obj)
                }
                ciborium::value::Value::Tag(_, value) => cbor_to_json(*value),
                _ => serde_json::Value::String(format!("{:?}", cbor)),
            }
        }

        let json_result = cbor_to_json(result);

        if let serde_json::Value::Array(ref arr) = json_result {
            if let Some(record) = arr.iter().next() {
                return Ok(record.clone());
            }
        }

        Ok(json_result)
    }
}

impl From<SurrealInsert> for Expression {
    fn from(insert: SurrealInsert) -> Self {
        insert.render()
    }
}

impl Expressive for SurrealInsert {
    fn expr(&self) -> Expression {
        self.render()
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
        assert!(rendered.contains("name = 'John'"));
        assert!(rendered.contains("age = 30"));
    }

    #[test]
    fn test_insert_with_id() {
        let insert = SurrealInsert::new("users")
            .with_id("john")
            .set_field("name", "John".to_string())
            .set_field("age", 30i64);

        let rendered = insert.preview();
        assert!(rendered.starts_with("CREATE users:john SET"));
        assert!(rendered.contains("name = 'John'"));
        assert!(rendered.contains("age = 30"));
    }

    #[test]
    #[cfg(feature = "decimal")]
    fn test_insert_with_large_decimal() {
        use rust_decimal::Decimal;

        let decimal = "999999999999999999.999999999999999999"
            .parse::<Decimal>()
            .unwrap();
        let insert = SurrealInsert::new("clients")
            .with_id("elon")
            .set_field("balance", decimal);

        let rendered = insert.preview();
        assert!(rendered.contains("CREATE clients:elon SET"));
        assert!(rendered.contains("balance = 999999999999999999.999999999999999999dec"));
    }

    #[test]
    fn test_insert_with_record_reference() {
        let insert = SurrealInsert::new("clients")
            .with_id("test")
            .set_field("bakery", "bakery:hill_valley".to_string());

        let rendered = insert.preview();
        assert!(rendered.contains("bakery = bakery:hill_valley"));
        // Should not be quoted since it's a record reference
        assert!(!rendered.contains("bakery = 'bakery:hill_valley'"));
    }

    #[test]
    #[cfg(feature = "decimal")]
    fn test_regular_decimal_no_suffix() {
        use rust_decimal::Decimal;

        let insert = SurrealInsert::new("clients")
            .with_id("test")
            .set_field("balance", Decimal::new(12345, 2));

        let rendered = insert.preview();
        // Decimal should render as string representation
        assert!(rendered.contains("balance = '123.45'"));
    }

    #[test]
    fn test_identifier_escaping() {
        let insert = SurrealInsert::new("SELECT") // Reserved keyword
            .set_field("FROM", "value".to_string()); // Reserved keyword as field

        let rendered = insert.preview();
        assert!(rendered.contains("CREATE ⟨SELECT⟩"));
        assert!(rendered.contains("⟨FROM⟩ = 'value'"));
    }
}
