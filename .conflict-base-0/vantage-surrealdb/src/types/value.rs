//! JSON Value type implementations for SurrealDB
//!
//! This module provides implementations of the SurrealType trait for serde_json::Value.

use crate::types::{SurrealType, SurrealTypeObjectMarker};
use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;

impl SurrealType for JsonValue {
    type Target = SurrealTypeObjectMarker;

    fn to_cbor(&self) -> CborValue {
        // Convert JSON value to CBOR using serde
        // TODO: silently replaces value with null, hope thats ok.
        ciborium::value::Value::serialized(self).unwrap_or(CborValue::Null)
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        // Convert CBOR value to JSON using serde
        cbor.deserialized().ok()
    }
}
