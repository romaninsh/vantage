#![cfg(feature = "serde")]

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use vantage_types::{IntoRecord, Record, TryFromRecord};

// Test structs - using automatic serde conversions (no macro needed)
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
struct User {
    name: String,
    age: i32,
}

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

        // Direct conversion using automatic serde implementation
        let json_record: Record<JsonValue> = user.into_record();

        assert_eq!(json_record.len(), 2);
        assert_eq!(json_record["name"], JsonValue::String("Alice".to_string()));
        assert_eq!(json_record["age"], JsonValue::Number(30.into()));

        // Can be passed to function expecting JSON record
        assert_eq!(process_json_record(json_record), 2);
    }

    #[test]
    fn test_record_to_struct_conversion() {
        let user = User {
            name: "Charlie".to_string(),
            age: 35,
        };

        // Round-trip: struct -> record -> struct using automatic serde implementations
        let record: Record<JsonValue> = user.clone().into_record();
        let restored_user: User = User::from_record(record).unwrap();

        assert_eq!(restored_user, user);
    }

    #[test]
    fn test_value_extraction() {
        let user = User {
            name: "David".to_string(),
            age: 40,
        };

        // Convert to typed record using automatic serde implementation
        let record: Record<JsonValue> = user.into_record();

        // Direct access to raw values
        assert_eq!(record["name"], JsonValue::String("David".to_string()));
        assert_eq!(record["age"], JsonValue::Number(40.into()));
    }

    #[test]
    fn test_nested_structures() {
        #[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
        struct Address {
            street: String,
            city: String,
        }

        #[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
        struct Person {
            name: String,
            address: Address,
        }

        let person = Person {
            name: "Bob".to_string(),
            address: Address {
                street: "123 Main St".to_string(),
                city: "Anytown".to_string(),
            },
        };

        // Convert nested structure using automatic serde implementation
        let record: Record<JsonValue> = person.clone().into_record();

        assert_eq!(record["name"], JsonValue::String("Bob".to_string()));
        assert!(record["address"].is_object());

        // Round-trip
        let restored_person: Person = Person::from_record(record).unwrap();
        assert_eq!(restored_person, person);
    }

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
        let record: Record<JsonValue> = user.clone().into_record();
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
