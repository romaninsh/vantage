//! `SurrealType` for strings and `char`.

use crate::types::{SurrealType, SurrealTypeStringMarker};
use ciborium::Value as CborValue;

impl SurrealType for String {
    type Target = SurrealTypeStringMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Text(self.clone())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Text(s) => Some(s),
            _ => None,
        }
    }
}

impl SurrealType for &'static str {
    type Target = SurrealTypeStringMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Text(self.to_string())
    }

    fn from_cbor(_cbor: CborValue) -> Option<Self> {
        // A decoded string can't be borrowed for 'static.
        None
    }
}

impl SurrealType for char {
    type Target = SurrealTypeStringMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Text(self.to_string())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Text(s) => s.chars().next(),
            _ => None,
        }
    }
}
