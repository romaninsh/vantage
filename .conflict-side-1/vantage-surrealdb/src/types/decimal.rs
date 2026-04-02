//! Decimal type implementations for SurrealDB
//!
//! This module provides implementations of the SurrealType trait for high-precision decimal types.

use crate::types::{SurrealType, SurrealTypeDecimalMarker};
use ciborium::Value as CborValue;

#[cfg(feature = "rust_decimal")]
impl SurrealType for rust_decimal::Decimal {
    type Target = SurrealTypeDecimalMarker;

    fn to_cbor(&self) -> CborValue {
        // SurrealDB uses Tag 10 for decimal strings
        CborValue::Tag(10, Box::new(CborValue::Text(self.to_string())))
    }

    fn from_cbor(cbor: CborValue) -> Option<Self> {
        match cbor {
            CborValue::Tag(10, boxed_value) => {
                if let CborValue::Text(s) = *boxed_value {
                    s.parse().ok()
                } else {
                    None
                }
            }
            CborValue::Text(s) => s.parse().ok(),
            _ => None,
        }
    }
}

// Only rust_decimal::Decimal is supported - no custom decimal types
