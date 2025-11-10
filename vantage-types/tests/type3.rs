use url::Url;
use vantage_types::{persistence, vantage_type_system};

// Generate Type3 system using the macro
vantage_type_system! {
    type_trait: Type3,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [String, Email]
}

// Override the macro-generated variant detection with our custom logic
impl Type3Variants {
    pub fn from_cbor(value: &ciborium::Value) -> Option<Self> {
        match value {
            ciborium::Value::Text(_) => Some(Type3Variants::String),
            ciborium::Value::Tag(1000, _) => Some(Type3Variants::Email),
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

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test1_direct_into() {
        let name = String::from("foobar");
        let website = Url::parse("https://example.com").unwrap();
        let email = Email::new("user", "example.com");

        let name = AnyType3::new(name);
        let website = AnyType3::new(website);
        let email = AnyType3::new(email);

        // Type is determined automatically
        assert_eq!(name.type_variant(), Some(Type3Variants::String));
        assert_eq!(website.type_variant(), Some(Type3Variants::String));
        assert_eq!(email.type_variant(), Some(Type3Variants::Email));

        // Now have only CBOR
        let name = name.value();
        let website = website.value();
        let email = email.value();

        let name = AnyType3::from_cbor(name).unwrap();
        let website = AnyType3::from_cbor(website).unwrap();
        let email = AnyType3::from_cbor(email).unwrap();

        let name: String = name.try_get().unwrap();
        let website: Url = website.try_get().unwrap();
        let email: Email = email.try_get().unwrap();

        assert_eq!(name, "foobar");
        assert_eq!(website.as_str(), "https://example.com/");
        assert_eq!(email, Email::new("user", "example.com"));
    }

    #[test]
    fn test1_record() {
        #[derive(PartialEq, Eq, Debug)]
        #[persistence(Type3)]
        struct Record {
            name: String,
            website: Url,
            email: Email,
        }

        let record = Record {
            name: "test".to_string(),
            website: Url::parse("https://example.com").unwrap(),
            email: Email::new("user", "example.com"),
        };

        let values = record.to_type3_map();
        assert_eq!(
            values.get("name").unwrap().type_variant(),
            Some(Type3Variants::String),
        );
        assert_eq!(
            values.get("website").unwrap().type_variant(),
            Some(Type3Variants::String),
        );
        assert_eq!(
            values.get("email").unwrap().type_variant(),
            Some(Type3Variants::Email),
        );

        // Test round-trip conversion
        let value = Record::from_type3_map(values).unwrap();
        assert_eq!(
            value,
            Record {
                name: "test".to_string(),
                website: Url::parse("https://example.com").unwrap(),
                email: Email::new("user", "example.com"),
            }
        );
    }
}
