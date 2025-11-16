use cbor_test::cbor::{from_cbor_value, to_cbor_value};
use cbor_test::{AnyType1, Email, Record, Type1};
use ciborium::value::Value as CborValue;
use std::collections::HashMap;
use url::Url;

#[test]
fn test_1_direct_cbor_conversion() {
    let name = "test".to_string();
    let website = Url::parse("https://example.com").unwrap();
    let email = Email::new("user", "example.com");

    let name_cbor = Type1::to_cbor(&name);
    let website_cbor = Type1::to_cbor(&website);
    let email_cbor = Type1::to_cbor(&email);

    assert_eq!(name_cbor, CborValue::Text("test".to_string()));
    assert_eq!(
        website_cbor,
        CborValue::Text("https://example.com/".to_string())
    );
    let expected_email = CborValue::Tag(
        1000,
        Box::new(CborValue::Array(vec![
            CborValue::Text("user".to_string()),
            CborValue::Text("example.com".to_string()),
        ])),
    );
    assert_eq!(email_cbor, expected_email);
}

#[test]
fn test_2_any_field_to_cbor() {
    let name = AnyType1::new("test".to_string());
    let website = AnyType1::new(Url::parse("https://example.com").unwrap());
    let email = AnyType1::new(Email::new("user", "example.com"));

    let name_cbor = name.to_cbor();
    let website_cbor = website.to_cbor();
    let email_cbor = email.to_cbor();

    assert_eq!(name_cbor, CborValue::Text("test".to_string()));
    assert_eq!(
        website_cbor,
        CborValue::Text("https://example.com/".to_string())
    );
    let expected_email = CborValue::Tag(
        1000,
        Box::new(CborValue::Array(vec![
            CborValue::Text("user".to_string()),
            CborValue::Text("example.com".to_string()),
        ])),
    );
    assert_eq!(email_cbor, expected_email);
}

#[test]
fn test_3_record_to_cbor_map() {
    let record = Record::new(
        "test".to_string(),
        Url::parse("https://example.com").unwrap(),
        Email::new("user", "example.com"),
    );

    let cbor_value = to_cbor_value(&record);

    // Verify the record was serialized to CBOR map with correct structure
    if let CborValue::Map(map) = cbor_value {
        assert_eq!(map.len(), 3);
        assert!(map
            .iter()
            .any(|(k, _)| k == &CborValue::Text("name".to_string())));
        assert!(map
            .iter()
            .any(|(k, _)| k == &CborValue::Text("website".to_string())));
        assert!(map
            .iter()
            .any(|(k, _)| k == &CborValue::Text("email".to_string())));
    } else {
        panic!("Expected CBOR map");
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
    any_map.insert("name".to_string(), AnyType1::new(record.name.clone()));
    any_map.insert("website".to_string(), AnyType1::new(record.website.clone()));
    any_map.insert("email".to_string(), AnyType1::new(record.email.clone()));

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
fn test_5_cbor_to_direct_types() {
    let name_cbor = CborValue::Text("test".to_string());
    let website_cbor = CborValue::Text("https://example.com/".to_string());
    let email_cbor = CborValue::Tag(
        1000,
        Box::new(CborValue::Array(vec![
            CborValue::Text("user".to_string()),
            CborValue::Text("example.com".to_string()),
        ])),
    );

    let name = String::from_cbor(&name_cbor).unwrap();
    let website = Url::from_cbor(&website_cbor).unwrap();
    let email = Email::from_cbor(&email_cbor).unwrap();

    assert_eq!(name, "test");
    assert_eq!(website.as_str(), "https://example.com/");
    assert_eq!(email, Email::new("user", "example.com"));
}

#[test]
fn test_6_cbor_to_any_field() {
    let name_cbor = CborValue::Text("test".to_string());
    let website_cbor = CborValue::Text("https://example.com/".to_string());
    let email_cbor = CborValue::Tag(
        1000,
        Box::new(CborValue::Array(vec![
            CborValue::Text("user".to_string()),
            CborValue::Text("example.com".to_string()),
        ])),
    );

    let name_any = AnyType1::from_cbor::<String>(&name_cbor).unwrap();
    let website_any = AnyType1::from_cbor::<Url>(&website_cbor).unwrap();
    let email_any = AnyType1::from_cbor::<Email>(&email_cbor).unwrap();

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
fn test_7_cbor_map_to_record() {
    let original = Record::new(
        "test".to_string(),
        Url::parse("https://example.com").unwrap(),
        Email::new("user", "example.com"),
    );

    let cbor_value = to_cbor_value(&original);
    let deserialized: Record = from_cbor_value(cbor_value);

    // Compare serialized forms instead of field values
    let mut original_buffer = Vec::new();
    ciborium::ser::into_writer(&original, &mut original_buffer).unwrap();
    let mut deserialized_buffer = Vec::new();
    ciborium::ser::into_writer(&deserialized, &mut deserialized_buffer).unwrap();

    assert_eq!(original_buffer, deserialized_buffer);
}

#[test]
fn test_8_any_field_map_to_record() {
    let mut any_map = HashMap::new();
    any_map.insert("name".to_string(), AnyType1::new("test".to_string()));
    any_map.insert(
        "website".to_string(),
        AnyType1::new(Url::parse("https://example.com").unwrap()),
    );
    any_map.insert(
        "email".to_string(),
        AnyType1::new(Email::new("user", "example.com")),
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
