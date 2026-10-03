//! Byte slice type implementation for the in-memory store.

use super::{MemoryType, MemoryTypeBytesMarker};
use ciborium::Value as CborValue;

impl MemoryType for Vec<u8> {
    type Target = MemoryTypeBytesMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Bytes(self.clone())
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Bytes(b) => Some(b),
            _ => None,
        }
    }
}
