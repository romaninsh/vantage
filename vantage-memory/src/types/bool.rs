//! Boolean type implementation for the in-memory store.

use super::{MemoryType, MemoryTypeBoolMarker};
use ciborium::Value as CborValue;

impl MemoryType for bool {
    type Target = MemoryTypeBoolMarker;

    fn to_cbor(&self) -> CborValue {
        CborValue::Bool(*self)
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Bool(b) => Some(b),
            _ => None,
        }
    }
}
