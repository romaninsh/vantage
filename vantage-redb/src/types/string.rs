//! String type implementations for redb.

use super::{RedbType, RedbTypeStringMarker};
use ciborium::Value as CborValue;

impl RedbType for String {
    type Target = RedbTypeStringMarker;

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

impl RedbType for char {
    type Target = RedbTypeStringMarker;

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
