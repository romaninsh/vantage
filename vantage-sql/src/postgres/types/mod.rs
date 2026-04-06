//! PostgreSQL Type System
//!
//! Defines type variants aligned with PostgreSQL's type system:
//!
//! - Bool       : BOOLEAN
//! - Int2       : SMALLINT, INT2
//! - Int4       : INTEGER, INT, INT4
//! - Int8       : BIGINT, INT8
//! - Float4     : REAL, FLOAT4
//! - Float8     : DOUBLE PRECISION, FLOAT8
//! - Text       : TEXT, VARCHAR(N), CHAR(N), NAME
//! - Bytea      : BYTEA
//!
//! Uses `serde_json::Value` as the underlying value type with type variant
//! tracking to prevent silent type confusion.

use serde_json::Value;
use vantage_types::vantage_type_system;

vantage_type_system! {
    type_trait: PostgresType,
    method_name: json,
    value_type: serde_json::Value,
    type_variants: [
        Null,
        Bool,       // bool
        Int2,       // i16
        Int4,       // i32
        Int8,       // i64
        Float4,     // f32
        Float8,     // f64
        Text,       // String, &str
        Bytea       // Vec<u8> — stored as base64 string
    ]
}

impl PostgresTypeVariants {
    /// Detect the type variant from a JSON value.
    pub fn from_json(value: &Value) -> Option<Self> {
        match value {
            Value::Null => Some(Self::Null),
            Value::Bool(_) => Some(Self::Bool),
            Value::Number(n) => {
                if n.is_i64() || n.is_u64() {
                    Some(Self::Int8)
                } else {
                    Some(Self::Float8)
                }
            }
            Value::String(_) => Some(Self::Text),
            Value::Object(obj) => {
                if obj.contains_key("bytea") {
                    Some(Self::Bytea)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

mod bool;
mod numbers;
mod string;
mod value;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_integer_round_trip() {
        let val = AnyPostgresType::new(42i64);
        assert_eq!(val.type_variant(), Some(PostgresTypeVariants::Int8));
        assert_eq!(val.try_get::<i64>(), Some(42));
    }

    #[test]
    fn test_text_round_trip() {
        let val = AnyPostgresType::new("hello".to_string());
        assert_eq!(val.type_variant(), Some(PostgresTypeVariants::Text));
        assert_eq!(val.try_get::<String>(), Some("hello".to_string()));
    }

    #[test]
    fn test_real_round_trip() {
        let val = AnyPostgresType::new(3.15f64);
        assert_eq!(val.type_variant(), Some(PostgresTypeVariants::Float8));
        assert_eq!(val.try_get::<f64>(), Some(3.15));
    }

    #[test]
    fn test_bool_round_trip() {
        let val = AnyPostgresType::new(true);
        assert_eq!(val.type_variant(), Some(PostgresTypeVariants::Bool));
        assert_eq!(val.try_get::<bool>(), Some(true));
        // Bool is a distinct variant — i64 extraction blocked by type boundary
        assert_eq!(val.try_get::<i64>(), None);
    }

    #[test]
    fn test_null_round_trip() {
        let val = AnyPostgresType::new(None::<i64>);
        assert_eq!(*val.value(), serde_json::Value::Null);
        assert_eq!(val.type_variant(), Some(PostgresTypeVariants::Int8));

        let direct = <Option<i64> as PostgresType>::from_json(serde_json::Value::Null);
        assert_eq!(direct, Some(None));

        assert_eq!(val.try_get::<Option<i64>>(), Some(None));
    }

    #[test]
    fn test_type_mismatch_text_as_integer() {
        let val = AnyPostgresType::new("hello".to_string());
        assert_eq!(val.try_get::<i64>(), None);
    }

    #[test]
    fn test_type_mismatch_integer_as_text() {
        let val = AnyPostgresType::new(42i64);
        assert_eq!(val.try_get::<String>(), None);
    }

    #[test]
    fn test_type_mismatch_real_as_integer() {
        let val = AnyPostgresType::new(3.15f64);
        assert_eq!(val.try_get::<i64>(), None);
    }

    #[test]
    fn test_i32_round_trip() {
        let val = AnyPostgresType::new(42i32);
        assert_eq!(val.type_variant(), Some(PostgresTypeVariants::Int4));
        assert_eq!(val.try_get::<i32>(), Some(42));
    }

    #[test]
    fn test_i16_round_trip() {
        let val = AnyPostgresType::new(42i16);
        assert_eq!(val.type_variant(), Some(PostgresTypeVariants::Int2));
        assert_eq!(val.try_get::<i16>(), Some(42));
    }
}
