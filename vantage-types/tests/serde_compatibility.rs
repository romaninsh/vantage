#![cfg(feature = "serde")]

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use vantage_types::{Record, TryFromRecord, TryIntoRecord};

// Test structs - using automatic serde conversions (no macro needed)
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
struct User {
    name: String,
    age: i32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversion_failure() {
        let mut record = Record::new();
        record.insert("name".to_string(), JsonValue::String("Invalid".to_string()));
        record.insert(
            "age".to_string(),
            JsonValue::String("not_a_number".to_string()),
        ); // Wrong type

        // Should fail to convert due to type mismatch
        let result = User::from_record(record);
        assert!(result.is_err());
    }

    #[test]
    fn test_record_json_value_conversions() {
        let user = User {
            name: "Eve".to_string(),
            age: 28,
        };

        // Struct -> Record -> JSON Value -> Record -> Struct
        let record: Record<JsonValue> = user.clone().try_into_record().unwrap();
        let json_value: JsonValue = record.into();
        let record_again: Record<JsonValue> = json_value.into();
        let user_again: User = User::from_record(record_again).unwrap();

        assert_eq!(user, user_again);
    }

    #[test]
    fn test_direct_json_object_to_record() {
        use serde_json::json;

        // Create a JSON object directly
        let json_obj = json!({
            "name": "Frank",
            "age": 45
        });

        // Convert to Record using new Into implementation
        let record: Record<JsonValue> = json_obj.into();

        assert_eq!(record["name"], JsonValue::String("Frank".to_string()));
        assert_eq!(record["age"], JsonValue::Number(45.into()));

        // Convert back to JSON object
        let json_again: JsonValue = record.into();
        assert!(json_again.is_object());
        assert_eq!(json_again["name"], "Frank");
        assert_eq!(json_again["age"], 45);
    }
}
