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
}
