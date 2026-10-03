//! String type implementations for the in-memory store.

use super::{MemoryType, MemoryTypeStringMarker};
use ciborium::Value as CborValue;

impl MemoryType for String {
    type Target = MemoryTypeStringMarker;

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

impl MemoryType for char {
    type Target = MemoryTypeStringMarker;

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
