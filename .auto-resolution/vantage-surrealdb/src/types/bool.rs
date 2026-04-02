//! Boolean type implementations for SurrealDB
//!
//! This module provides implementations of the SurrealType trait for boolean types.

use crate::types::{SurrealType, SurrealTypeBoolMarker};
use ciborium::Value as CborValue;

impl SurrealType for bool {
    type Target = SurrealTypeBoolMarker;

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
