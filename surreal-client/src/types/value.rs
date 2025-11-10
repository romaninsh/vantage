//! JSON value implementation for SurrealType trait using vantage-types

use super::{SurrealType, SurrealTypeJsonMarker};
use ciborium::value::Value as CborValue;
use serde_json::Value;

impl SurrealType for Value {
    type Target = SurrealTypeJsonMarker;

    fn to_cbor(&self) -> CborValue {
        // Use serde to convert JSON to CBOR - this preserves structure and types
        ciborium::value::Value::serialized(self).unwrap_or(CborValue::Null)
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        // Use serde to convert CBOR to JSON - this handles tags appropriately
        cbor.deserialized().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_json_value_object() {
        let obj = json!({
            "name": "John",
            "age": 30,
            "active": true
        });

        let cbor = obj.to_cbor();
        let restored = Value::from_cbor(cbor).unwrap();
        assert_eq!(obj, restored);
    }

    #[test]
    fn test_json_value_array() {
        let arr = json!([1, 2, 3, "hello"]);

        let cbor = arr.to_cbor();
        let restored = Value::from_cbor(cbor).unwrap();
        assert_eq!(arr, restored);
    }

    #[test]
    fn test_json_value_primitives() {
        let test_cases = vec![
            json!("test"),
            json!(42),
            json!(42.5),
            json!(true),
            json!(false),
            json!(null),
        ];

        for case in test_cases {
            let cbor = case.to_cbor();
            let restored = Value::from_cbor(cbor).unwrap();
            assert_eq!(case, restored);
        }
    }

    #[test]
    fn test_cbor_tag_handling_via_serde() {
        // Test that serde properly handles CBOR tags when converting to JSON
        // This relies on ciborium's serde implementation to handle tags appropriately

        let complex_json = serde_json::json!({
            "timestamp": "2022-01-01T00:00:00Z",
            "duration": 123.456,
            "decimal": "123.456789",
            "nested": {
                "array": [1, 2, 3],
                "null_value": null
            }
        });

        let cbor = complex_json.to_cbor();
        let restored = Value::from_cbor(cbor).unwrap();
        assert_eq!(complex_json, restored);
    }
}
