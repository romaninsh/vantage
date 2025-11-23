use serde_json::Value as JsonValue;
use vantage_types::{persistence, vantage_type_system, IntoRecord, Record, TryFromRecord};

// Create a CBOR-based type system
vantage_type_system! {
    type_trait: CborType,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [String, Int]
}

// Create a JSON-based type system
vantage_type_system! {
    type_trait: JsonType,
    method_name: json,
    value_type: serde_json::Value,
    type_variants: [String, Int]
}

// Implement CBOR type system
impl CborTypeVariants {
    pub fn from_cbor(value: &ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Text(_) => Some(CborTypeVariants::String),
            ciborium::Value::Integer(_) => Some(CborTypeVariants::Int),
            _ => None,
        }
    }
}

impl CborType for String {
    type Target = CborTypeStringMarker;
    fn to_cbor(&self) -> ciborium::Value {
        ciborium::Value::Text(self.clone())
    }
    fn from_cbor(value: ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Text(s) => Some(s),
            _ => None,
        }
    }
}

impl CborType for i32 {
    type Target = CborTypeIntMarker;
    fn to_cbor(&self) -> ciborium::Value {
        ciborium::Value::Integer((*self).into())
    }
    fn from_cbor(value: ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Integer(i) => i.try_into().ok(),
            _ => None,
        }
    }
}

// Implement JSON type system
impl JsonTypeVariants {
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::String(_) => Some(JsonTypeVariants::String),
            serde_json::Value::Number(n) if n.is_i64() => Some(JsonTypeVariants::Int),
            _ => None,
        }
    }
}

impl JsonType for String {
    type Target = JsonTypeStringMarker;
    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::String(self.clone())
    }
    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::String(s) => Some(s),
            _ => None,
        }
    }
}

impl JsonType for i32 {
    type Target = JsonTypeIntMarker;
    fn to_json(&self) -> serde_json::Value {
        serde_json::Value::Number((*self).into())
    }
    fn from_json(value: serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::Number(n) => n.as_i64().and_then(|i| i.try_into().ok()),
            _ => None,
        }
    }
}

// Test structs
#[derive(Debug, PartialEq, Clone)]
#[persistence(CborType)]
#[persistence(JsonType)]
struct User {
    name: String,
    age: i32,
}

// User already has IntoRecord/TryFromRecord from #[persistence] macros
// No additional implementations needed

// Mock function that expects JSON values
fn process_json_record(record: Record<JsonValue>) -> usize {
    record.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_type_conversion() {
        let user = User {
            name: "Alice".to_string(),
            age: 30,
        };

        // Convert to raw JSON values
        let typed_record: Record<AnyJsonType> = user.clone().into_record();
        let json_record: Record<JsonValue> = typed_record.into_record();

        assert_eq!(json_record.len(), 2);
        assert_eq!(json_record["name"], JsonValue::String("Alice".to_string()));
        assert_eq!(json_record["age"], JsonValue::Number(30.into()));

        // Can be passed to function expecting JSON record
        assert_eq!(process_json_record(json_record), 2);
    }

    #[test]
    fn test_cbor_type_conversion() {
        let user = User {
            name: "Bob".to_string(),
            age: 25,
        };

        // Test CBOR type conversion
        let cbor_record: Record<AnyCborType> = user.into_record();

        assert_eq!(cbor_record.len(), 2);

        // Verify CBOR types
        assert_eq!(
            cbor_record["name"].type_variant(),
            Some(CborTypeVariants::String)
        );
        assert_eq!(
            cbor_record["age"].type_variant(),
            Some(CborTypeVariants::Int)
        );

        // Extract values to verify they're correct
        let name: String = cbor_record["name"].try_get().unwrap();
        let age: i32 = cbor_record["age"].try_get().unwrap();
        assert_eq!(name, "Bob");
        assert_eq!(age, 25);
    }

    #[test]
    fn test_record_to_struct_conversion() {
        let user = User {
            name: "Charlie".to_string(),
            age: 35,
        };

        // Round-trip: struct -> record -> struct using IntoRecord/TryFromRecord
        let record: Record<AnyJsonType> = user.clone().into_record();
        let restored_user: User = User::from_record(record).unwrap();

        assert_eq!(
            restored_user,
            User {
                name: "Charlie".to_string(),
                age: 35,
            }
        );
    }

    #[test]
    fn test_value_extraction() {
        let user = User {
            name: "David".to_string(),
            age: 40,
        };

        // Convert to typed record using IntoRecord
        let typed_record: Record<AnyJsonType> = user.into_record();

        // Extract raw values
        let raw_record: Record<JsonValue> = typed_record.into_record();

        assert_eq!(raw_record["name"], JsonValue::String("David".to_string()));
        assert_eq!(raw_record["age"], JsonValue::Number(40.into()));
    }
}
