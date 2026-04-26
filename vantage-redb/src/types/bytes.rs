//! Byte slice type implementation for redb.

use super::{RedbType, RedbTypeBytesMarker};
use ciborium::Value as CborValue;

impl RedbType for Vec<u8> {
    type Target = RedbTypeBytesMarker;

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
