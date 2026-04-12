//! rust_decimal::Decimal implementation for SQLite.
//!
//! Stored as CBOR Tag(10, Text("string_representation")).
//! SQLite has NUMERIC affinity but stores decimals as text or real internally.
//! `from_cbor` accepts Tag(10), plain Text, Integer, and Float for flexibility.

use super::{SqliteType, SqliteTypeNumericMarker};
use ciborium::Value;
use rust_decimal::Decimal;

impl SqliteType for Decimal {
    type Target = SqliteTypeNumericMarker;

    fn to_cbor(&self) -> Value {
        Value::Tag(10, Box::new(Value::Text(self.to_string())))
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Tag(10, inner) => {
                if let Value::Text(s) = *inner {
                    s.parse().ok()
                } else {
                    None
                }
            }
            Value::Text(s) => s.parse().ok(),
            Value::Integer(i) => {
                let n = i128::from(i);
                Some(Decimal::from(n))
            }
            Value::Float(f) => Decimal::try_from(f).ok(),
            _ => None,
        }
    }
}
