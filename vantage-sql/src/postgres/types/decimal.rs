//! rust_decimal::Decimal implementation for PostgreSQL.
//!
//! Stored as CBOR Tag(10, Text("string_representation")).
//! `from_cbor` accepts Tag(10), plain Text, Integer, and Float for flexibility.

use super::{PostgresType, PostgresTypeDecimalMarker};
use ciborium::Value;
use rust_decimal::Decimal;

impl PostgresType for Decimal {
    type Target = PostgresTypeDecimalMarker;

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
