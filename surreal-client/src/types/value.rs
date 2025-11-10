//! JSON value implementation for SurrealType trait using vantage-types

use super::{SurrealType, SurrealTypeJsonMarker};
use ciborium::value::Value as CborValue;
use serde_json::Value;

impl SurrealType for Value {
    type Target = SurrealTypeJsonMarker;

    fn to_cbor(&self) -> CborValue {
        // Convert JSON Value to CBOR Value
        match self {
            Value::Null => CborValue::Null,
            Value::Bool(b) => CborValue::Bool(*b),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    CborValue::Integer(i.into())
                } else if let Some(f) = n.as_f64() {
                    CborValue::Float(f)
                } else {
                    CborValue::Null
                }
            }
            Value::String(s) => CborValue::Text(s.clone()),
            Value::Array(arr) => {
                let cbor_array: Vec<CborValue> = arr.iter().map(|v| v.to_cbor()).collect();
                CborValue::Array(cbor_array)
            }
            Value::Object(obj) => {
                let cbor_map: Vec<(CborValue, CborValue)> = obj
                    .iter()
                    .map(|(k, v)| (CborValue::Text(k.clone()), v.to_cbor()))
                    .collect();
                CborValue::Map(cbor_map)
            }
        }
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        // Convert CBOR Value to JSON Value
        match cbor {
            CborValue::Null => Some(Value::Null),
            CborValue::Bool(b) => Some(Value::Bool(b)),
            CborValue::Integer(i) => {
                let val = i128::from(i);
                if let Ok(i64_val) = i64::try_from(val) {
                    Some(Value::Number(i64_val.into()))
                } else {
                    // Fall back to string for very large integers
                    Some(Value::String(val.to_string()))
                }
            }
            CborValue::Float(f) => {
                if let Some(num) = serde_json::Number::from_f64(f) {
                    Some(Value::Number(num))
                } else {
                    None
                }
            }
            CborValue::Text(s) => Some(Value::String(s)),
            CborValue::Bytes(b) => {
                // Convert bytes to hex string
                Some(Value::String(hex::encode(b)))
            }
            CborValue::Array(arr) => {
                let json_array: Option<Vec<Value>> =
                    arr.into_iter().map(|v| Value::from_cbor(v)).collect();
                json_array.map(Value::Array)
            }
            CborValue::Map(map) => {
                let mut json_object = serde_json::Map::new();
                for (k, v) in map {
                    let key = match k {
                        CborValue::Text(s) => s,
                        CborValue::Integer(i) => i128::from(i).to_string(),
                        _ => format!("{:?}", k),
                    };
                    if let Some(value) = Value::from_cbor(v) {
                        json_object.insert(key, value);
                    }
                }
                Some(Value::Object(json_object))
            }
            CborValue::Tag(_tag, boxed_value) => {
                // For tagged values, try to extract the inner value
                Value::from_cbor(*boxed_value)
            }
            _ => None,
        }
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
}
