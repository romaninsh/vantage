use url::Url;
use vantage_types::{persistence, vantage_type_system, IntoRecord, TryFromRecord};

// Generate Type3 system using the macro with None type for optionals
vantage_type_system! {
    type_trait: Type3,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [String, Email, Null]
}

// Override the macro-generated variant detection with our custom logic
impl Type3Variants {
    pub fn from_cbor(value: &ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Text(_) => Some(Type3Variants::String),
            ciborium::Value::Tag(1000, _) => Some(Type3Variants::Email),
            ciborium::Value::Tag(6, _) => None,
            _ => None,
        }
    }
}

impl Type3 for String {
    type Target = Type3StringMarker;
    fn to_cbor(&self) -> ciborium::Value {
        ciborium::Value::Text(self.clone())
    }
    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Text(s) => Some(s.clone()),
            _ => None,
        }
    }
}

impl Type3 for Url {
    type Target = Type3StringMarker;
    fn to_cbor(&self) -> ciborium::Value {
        ciborium::Value::Text(self.to_string())
    }
    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Text(s) => Url::parse(&s).ok(),
            _ => None,
        }
    }
}

// Custom Email struct
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct Email {
    pub name: String,
    pub domain: String,
}

impl Email {
    pub fn new(name: &str, domain: &str) -> Self {
        Self {
            name: name.to_string(),
            domain: domain.to_string(),
        }
    }
}
impl Type3 for Email {
    type Target = Type3EmailMarker;
    fn to_cbor(&self) -> ciborium::value::Value {
        // Use custom CBOR tag 1000 with array [name, domain]
        let array = vec![
            ciborium::value::Value::Text(self.name.clone()),
            ciborium::value::Value::Text(self.domain.clone()),
        ];
        ciborium::value::Value::Tag(1000, Box::new(ciborium::value::Value::Array(array)))
    }

    fn from_cbor(cbor: ciborium::value::Value) -> Option<Self> {
        let ciborium::value::Value::Tag(1000, boxed_value) = cbor else {
            return None;
        };
        let ciborium::value::Value::Array(arr) = boxed_value.as_ref() else {
            return None;
        };

        let name = match arr.get(0)? {
            ciborium::value::Value::Text(s) => s,
            _ => return None,
        };

        let domain = match arr.get(1)? {
            ciborium::value::Value::Text(s) => s,
            _ => return None,
        };

        Some(Email::new(name, domain))
    }
}

// Implement Option<String> support
impl Type3 for Option<String> {
    type Target = Type3StringMarker; // We'll use String marker as primary type

    fn to_cbor(&self) -> ciborium::Value {
        match self {
            Some(s) => ciborium::Value::Text(s.clone()),
            None => ciborium::Value::Tag(6, Box::new(ciborium::Value::Null)),
        }
    }

    fn from_cbor(cbor: ciborium::Value) -> Option<Self> {
        match cbor {
            ciborium::Value::Tag(6, _) => Some(None),
            ciborium::Value::Text(s) => Some(Some(s)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {

    use indexmap::IndexMap;

    use super::*;

    #[test]
    fn test_optional_types() {
        // Test Some value
        let some_name: Option<String> = Some("John".to_string());
        let any_some = AnyType3::new(some_name.clone());

        // Should use String variant (since we use StringMarker as Target)
        assert_eq!(any_some.type_variant(), Some(Type3Variants::String));

        // Test round-trip
        let cbor_value = any_some.value();
        let restored_any = AnyType3::from_cbor(cbor_value).unwrap();
        let restored_some: Option<String> = restored_any.try_get().unwrap();
        assert_eq!(some_name, restored_some);

        // Test None value
        let none_name: Option<String> = None;
        let any_none = AnyType3::new(none_name.clone());

        // Should use String variant (since we use StringMarker as Target)
        assert_eq!(any_none.type_variant(), Some(Type3Variants::String));

        // Test None round-trip
        let cbor_none = any_none.value();
        let restored_none_any = AnyType3::from_cbor(cbor_none).unwrap();

        // Persistence looses our type info - that's expected
        assert_eq!(restored_none_any.type_variant(), None);

        // Cannot restore into string
        assert!(restored_none_any.try_get::<String>().is_none());

        // Technically successful, but is none regardless
        assert_eq!(restored_none_any.try_get::<Option<String>>(), Some(None));
    }

    #[test]
    fn test_record_with_optionals() {
        #[derive(PartialEq, Eq, Debug, Clone)]
        #[persistence(Type3)]
        struct UserRecord {
            name: String,
            nickname: Option<String>,
            email: Email,
        }

        // Test with Some nickname
        let record_with_nick = UserRecord {
            name: "John Doe".to_string(),
            nickname: Some("Johnny".to_string()),
            email: Email::new("john", "example.com"),
        };

        let values: vantage_types::Record<AnyType3> = record_with_nick.clone().into_record();
        let restored = UserRecord::from_record(values).unwrap();
        assert_eq!(record_with_nick, restored);

        // Test with None nickname
        let record_no_nick = UserRecord {
            name: "Jane Doe".to_string(),
            nickname: None,
            email: Email::new("jane", "example.com"),
        };

        let values_none: vantage_types::Record<AnyType3> = record_no_nick.clone().into_record();
        let cborize = values_none
            .into_iter()
            .map(|(k, v)| (k, v.value().clone()))
            .map(|(k, v)| (k, AnyType3::from_cbor(&v).unwrap()))
            .collect::<IndexMap<_, _>>();
        assert_eq!(
            cborize.get("nickname").unwrap().type_variant(),
            None, // Null type as persistence didn't know it
        );

        // Test round-trip with None
        let restored_none = UserRecord::from_record(cborize.into()).unwrap();
        assert_eq!(record_no_nick, restored_none);
    }
}
