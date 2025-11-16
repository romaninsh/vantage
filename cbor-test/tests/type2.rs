use cbor_test::{AnyType2, Email, Record, Type2};
use serde_json::{json, Value as JsonValue};
use std::collections::HashMap;
use url::Url;

#[test]
fn test_1_direct_json_conversion() {
    let name = "test".to_string();
    let website = Url::parse("https://example.com").unwrap();
    let email = Email::new("user", "example.com");

    let name_json = Type2::to_json(&name);
    let website_json = Type2::to_json(&website);
    let email_json = Type2::to_json(&email);

    assert_eq!(name_json, JsonValue::String("test".to_string()));
    assert_eq!(
        website_json,
        JsonValue::String("https://example.com/".to_string())
    );
    let expected_email = json!({
        "name": "user",
        "domain": "example.com"
    });
    assert_eq!(email_json, expected_email);
}

#[test]
fn test_2_any_field_to_json() {
    let name = AnyType2::new("test".to_string());
    let website = AnyType2::new(Url::parse("https://example.com").unwrap());
    let email = AnyType2::new(Email::new("user", "example.com"));

    let name_json = name.to_json();
    let website_json = website.to_json();
    let email_json = email.to_json();

    assert_eq!(name_json, JsonValue::String("test".to_string()));
    assert_eq!(
        website_json,
        JsonValue::String("https://example.com/".to_string())
    );
    let expected_email = json!({
        "name": "user",
        "domain": "example.com"
    });
    assert_eq!(email_json, expected_email);
}

#[test]
fn test_3_record_to_json_map() {
    let record = Record::new(
        "test".to_string(),
        Url::parse("https://example.com").unwrap(),
        Email::new("user", "example.com"),
    );

    let json_value = serde_json::to_value(&record).unwrap();

    // Verify the record was serialized to JSON object with correct structure
    if let JsonValue::Object(obj) = json_value {
        assert_eq!(obj.len(), 3);
        assert!(obj.contains_key("name"));
        assert!(obj.contains_key("website"));
        assert!(obj.contains_key("email"));
    } else {
        panic!("Expected JSON object");
    }
}

#[test]
fn test_4_record_to_any_field_map() {
    let record = Record::new(
        "test".to_string(),
        Url::parse("https://example.com").unwrap(),
        Email::new("user", "example.com"),
    );

    let mut any_map = HashMap::new();
    any_map.insert("name".to_string(), AnyType2::new(record.name.clone()));
    any_map.insert("website".to_string(), AnyType2::new(record.website.clone()));
    any_map.insert("email".to_string(), AnyType2::new(record.email.clone()));

    assert_eq!(
        any_map
            .get("name")
            .unwrap()
            .downcast_ref::<String>()
            .unwrap(),
        "test"
    );
    assert_eq!(
        any_map
            .get("website")
            .unwrap()
            .downcast_ref::<Url>()
            .unwrap()
            .as_str(),
        "https://example.com/"
    );
    assert_eq!(
        any_map
            .get("email")
            .unwrap()
            .downcast_ref::<Email>()
            .unwrap(),
        &Email::new("user", "example.com")
    );
}

#[test]
fn test_5_json_to_direct_types() {
    let name_json = JsonValue::String("test".to_string());
    let website_json = JsonValue::String("https://example.com/".to_string());
    let email_json = json!({
        "name": "user",
        "domain": "example.com"
    });

    let name = String::from_json(&name_json).unwrap();
    let website = Url::from_json(&website_json).unwrap();
    let email = Email::from_json(&email_json).unwrap();

    assert_eq!(name, "test");
    assert_eq!(website.as_str(), "https://example.com/");
    assert_eq!(email, Email::new("user", "example.com"));
}

#[test]
fn test_6_json_to_any_field() {
    let name_json = JsonValue::String("test".to_string());
    let website_json = JsonValue::String("https://example.com/".to_string());
    let email_json = json!({
        "name": "user",
        "domain": "example.com"
    });

    let name_any = AnyType2::from_json::<String>(&name_json).unwrap();
    let website_any = AnyType2::from_json::<Url>(&website_json).unwrap();
    let email_any = AnyType2::from_json::<Email>(&email_json).unwrap();

    assert_eq!(name_any.downcast_ref::<String>().unwrap(), "test");
    assert_eq!(
        website_any.downcast_ref::<Url>().unwrap().as_str(),
        "https://example.com/"
    );
    assert_eq!(
        email_any.downcast_ref::<Email>().unwrap(),
        &Email::new("user", "example.com")
    );
}

#[test]
fn test_7_json_map_to_record() {
    let original = Record::new(
        "test".to_string(),
        Url::parse("https://example.com").unwrap(),
        Email::new("user", "example.com"),
    );

    let json_value = serde_json::to_value(&original).unwrap();
    let deserialized: Record = serde_json::from_value(json_value).unwrap();

    // Compare serialized forms instead of field values
    let original_json = serde_json::to_string(&original).unwrap();
    let deserialized_json = serde_json::to_string(&deserialized).unwrap();

    assert_eq!(original_json, deserialized_json);
}

#[test]
fn test_8_any_field_map_to_record() {
    let mut any_map = HashMap::new();
    any_map.insert("name".to_string(), AnyType2::new("test".to_string()));
    any_map.insert(
        "website".to_string(),
        AnyType2::new(Url::parse("https://example.com").unwrap()),
    );
    any_map.insert(
        "email".to_string(),
        AnyType2::new(Email::new("user", "example.com")),
    );

    let name = any_map
        .get("name")
        .unwrap()
        .downcast_ref::<String>()
        .unwrap()
        .clone();
    let website = any_map
        .get("website")
        .unwrap()
        .downcast_ref::<Url>()
        .unwrap()
        .clone();
    let email = any_map
        .get("email")
        .unwrap()
        .downcast_ref::<Email>()
        .unwrap()
        .clone();
    let record = Record::new(name, website, email);

    assert_eq!(record.name, "test");
    assert_eq!(record.website.as_str(), "https://example.com/");
    assert_eq!(record.email, Email::new("user", "example.com"));
}
