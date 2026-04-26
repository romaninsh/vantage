//! Boolean type implementation for redb.

use super::{RedbType, RedbTypeBoolMarker};
use ciborium::Value as CborValue;

impl RedbType for bool {
    type Target = RedbTypeBoolMarker;

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
