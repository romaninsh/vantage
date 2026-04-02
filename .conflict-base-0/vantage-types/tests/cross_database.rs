use rust_decimal::Decimal;
use serde_json::Value as JsonValue;
use vantage_types::{entity, vantage_type_system, IntoRecord, TryFromRecord};

// SurrealDB type system with CBOR
vantage_type_system! {
    type_trait: SurrealType,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [String, Decimal, RId]
}

// PostgreSQL type system with JSON
vantage_type_system! {
    type_trait: PostgresType,
    method_name: json,
    value_type: serde_json::Value,
    type_variants: [String, Decimal, Uuid]
}

// Custom RId type for SurrealDB
#[derive(Debug, Clone, PartialEq)]
pub struct RId(String);

impl RId {
    pub fn new(table: &str, id: &str) -> Self {
        Self(format!("{}:{}", table, id))
    }
}

impl SurrealType for RId {
    type Target = SurrealTypeRIdMarker;

    fn to_cbor(&self) -> ciborium::Value {
        ciborium::Value::Tag(100, Box::new(ciborium::Value::Text(self.0.clone())))
    }

    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Tag(100, boxed_value) => {
                if let ciborium::Value::Text(s) = *boxed_value {
                    Some(RId(s))
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

// Custom Uuid type for PostgreSQL
#[derive(Debug, Clone, PartialEq)]
pub struct Uuid(String);

impl Uuid {
    pub fn new() -> Self {
        Self("550e8400-e29b-41d4-a716-446655440000".to_string()) // Mock UUID
    }
}

impl Default for Uuid {
    fn default() -> Self {
        Self::new()
    }
}

impl PostgresType for Uuid {
    type Target = PostgresTypeUuidMarker;

    fn to_json(&self) -> JsonValue {
        JsonValue::String(self.0.clone())
    }

    fn from_json(value: JsonValue) -> Option<Self> {
        match value {
            JsonValue::String(s) => Some(Uuid(s)),
            _ => None,
        }
    }
}

// Implement String for both type systems
impl SurrealType for String {
    type Target = SurrealTypeStringMarker;

    fn to_cbor(&self) -> ciborium::Value {
        ciborium::Value::Text(self.clone())
    }

    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Text(s) => Some(s),
            _ => None,
        }
    }
}

impl PostgresType for String {
    type Target = PostgresTypeStringMarker;

    fn to_json(&self) -> JsonValue {
        JsonValue::String(self.clone())
    }

    fn from_json(value: JsonValue) -> Option<Self> {
        match value {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }
}

// Implement Decimal for both type systems
impl SurrealType for Decimal {
    type Target = SurrealTypeDecimalMarker;

    fn to_cbor(&self) -> ciborium::Value {
        ciborium::Value::Tag(200, Box::new(ciborium::Value::Text(self.to_string())))
    }

    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Tag(200, boxed_value) => {
                if let ciborium::Value::Text(s) = *boxed_value {
                    s.parse().ok()
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl PostgresType for Decimal {
    type Target = PostgresTypeDecimalMarker;

    fn to_json(&self) -> JsonValue {
        serde_json::json!({"decimal": self.to_string()})
    }

    fn from_json(value: JsonValue) -> Option<Self> {
        match value {
            JsonValue::Object(obj) => {
                if let Some(JsonValue::String(decimal_str)) = obj.get("decimal") {
                    decimal_str.parse().ok()
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

// Implement variant detection
impl SurrealTypeVariants {
    pub fn from_cbor(value: &ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Text(_) => Some(SurrealTypeVariants::String),
            ciborium::Value::Tag(100, _) => Some(SurrealTypeVariants::RId),
            ciborium::Value::Tag(200, _) => Some(SurrealTypeVariants::Decimal),
            _ => None,
        }
    }
}

impl PostgresTypeVariants {
    pub fn from_json(value: &JsonValue) -> Option<Self> {
        match value {
            JsonValue::String(_) => {
                // Need to distinguish between UUID and regular string
                // For simplicity, assume UUID format check
                if value.as_str().unwrap().contains('-') {
                    Some(PostgresTypeVariants::Uuid)
                } else {
                    Some(PostgresTypeVariants::String)
                }
            }
            JsonValue::Object(obj) => {
                if obj.contains_key("decimal") {
                    Some(PostgresTypeVariants::Decimal)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

// User struct with dual persistence support
#[derive(Debug, PartialEq, Clone)]
#[entity(SurrealType)]
#[entity(PostgresType)]
struct User {
    name: String,
    balance: Decimal,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cross_database_persistence() {
        let user = User {
            name: "John Doe".to_string(),
            balance: Decimal::from_str_exact("1234.56").unwrap(),
        };

        // Store to SurrealDB format
        let surreal_storage: vantage_types::Record<AnySurrealType> = user.clone().into_record();

        // Verify SurrealDB storage format
        let name_cbor = surreal_storage.get("name").unwrap().value();
        assert!(matches!(name_cbor, ciborium::Value::Text(_)));

        let balance_cbor = surreal_storage.get("balance").unwrap().value();
        assert!(matches!(balance_cbor, ciborium::Value::Tag(200, _)));

        // Store to PostgreSQL format
        let postgres_storage: vantage_types::Record<AnyPostgresType> = user.clone().into_record();

        // Verify PostgreSQL storage format
        let name_json = postgres_storage.get("name").unwrap().value();
        assert_eq!(name_json, &JsonValue::String("John Doe".to_string()));

        let balance_json = postgres_storage.get("balance").unwrap().value();
        assert_eq!(balance_json, &serde_json::json!({"decimal": "1234.56"}));

        // Test round-trip for both systems
        let restored_surreal = User::from_record(surreal_storage).unwrap();
        assert_eq!(user, restored_surreal);

        let restored_postgres = User::from_record(postgres_storage).unwrap();
        assert_eq!(user, restored_postgres);
    }

    #[test]
    fn test_type_system_isolation() {
        // Create values using different type systems
        let surreal_name = AnySurrealType::new("Alice".to_string());
        let postgres_name = AnyPostgresType::new("Alice".to_string());

        let surreal_balance = AnySurrealType::new(Decimal::from_str_exact("999.99").unwrap());
        let postgres_balance = AnyPostgresType::new(Decimal::from_str_exact("999.99").unwrap());

        // Verify different storage formats
        assert!(matches!(surreal_name.value(), ciborium::Value::Text(_)));
        assert!(matches!(postgres_name.value(), JsonValue::String(_)));

        assert!(matches!(
            surreal_balance.value(),
            ciborium::Value::Tag(200, _)
        ));
        assert_eq!(
            postgres_balance.value(),
            &serde_json::json!({"decimal": "999.99"})
        );

        // Verify type variants
        assert_eq!(
            surreal_name.type_variant(),
            Some(SurrealTypeVariants::String)
        );
        assert_eq!(
            postgres_name.type_variant(),
            Some(PostgresTypeVariants::String)
        );
    }
}
