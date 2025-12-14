use crate::Email;
use crate::Type1;
use url::Url;

// Implementations for String
impl Type1 for String {
    fn to_cbor(&self) -> ciborium::value::Value {
        ciborium::value::Value::Text(self.clone())
    }

    fn from_cbor(cbor: &ciborium::value::Value) -> Option<Self> {
        match cbor {
            ciborium::value::Value::Text(s) => Some(s.clone()),
            _ => None,
        }
    }
}

// Implementations for Url
impl Type1 for Url {
    fn to_cbor(&self) -> ciborium::value::Value {
        ciborium::value::Value::Text(self.to_string())
    }

    fn from_cbor(cbor: &ciborium::value::Value) -> Option<Self> {
        match cbor {
            ciborium::value::Value::Text(s) => Url::parse(s).ok(),
            _ => None,
        }
    }
}

// Implementations for Email
impl Type1 for Email {
    fn to_cbor(&self) -> ciborium::value::Value {
        // Use custom CBOR tag 1000 with array [name, domain]
        let array = vec![
            ciborium::value::Value::Text(self.name.clone()),
            ciborium::value::Value::Text(self.domain.clone()),
        ];
        ciborium::value::Value::Tag(1000, Box::new(ciborium::value::Value::Array(array)))
    }

    fn from_cbor(cbor: &ciborium::value::Value) -> Option<Self> {
        let ciborium::value::Value::Tag(1000, boxed_value) = cbor else {
            return None;
        };
        let ciborium::value::Value::Array(arr) = boxed_value.as_ref() else {
            return None;
        };

        let name = match arr.first()? {
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
