//! SQLite Type System
//!
//! Defines type variants aligned with SQLite's affinity system:
//!
//! - INTEGER affinity: INTEGER, INT, TINYINT, SMALLINT, MEDIUMINT, BIGINT
//! - TEXT    affinity: TEXT, VARCHAR(N), CHAR(N), CLOB, NVARCHAR(N)
//! - REAL    affinity: REAL, FLOAT, DOUBLE, DOUBLE PRECISION
//! - NUMERIC affinity: NUMERIC, DECIMAL(P,S), BOOLEAN, DATE, DATETIME
//! - BLOB    affinity: BLOB
//! - ANY     (STRICT): ANY
//!
//! Uses `serde_json::Value` as the underlying value type with type variant
//! tracking to prevent silent type confusion.

use serde_json::Value;
use vantage_types::vantage_type_system;

vantage_type_system! {
    type_trait: SqliteType,
    method_name: json,
    value_type: serde_json::Value,
    type_variants: [
        Null,
        Bool,       // bool — stored as 0/1 on disk, but distinct from Integer at type level
        Integer,    // i8, i16, i32, i64
        Text,       // String, &str
        Real,       // f32, f64
        Numeric,    // Decimal — stored as {"numeric": "string"} to preserve precision
        Blob        // Vec<u8> — stored as base64 string
    ]
}

impl SqliteTypeVariants {
    /// Detect the type variant from a JSON value.
    pub fn from_json(value: &Value) -> Option<Self> {
        match value {
            Value::Null => Some(Self::Null),
            Value::Bool(_) => Some(Self::Bool),
            Value::Number(n) => {
                if n.is_i64() || n.is_u64() {
                    Some(Self::Integer)
                } else {
                    Some(Self::Real)
                }
            }
            Value::String(_) => Some(Self::Text),
            Value::Object(obj) => {
                if obj.contains_key("numeric") {
                    Some(Self::Numeric)
                } else if obj.contains_key("blob") {
                    Some(Self::Blob)
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
        let val = AnySqliteType::new(42i64);
        assert_eq!(val.type_variant(), Some(SqliteTypeVariants::Integer));
        assert_eq!(val.try_get::<i64>(), Some(42));
    }

    #[test]
    fn test_text_round_trip() {
        let val = AnySqliteType::new("hello".to_string());
        assert_eq!(val.type_variant(), Some(SqliteTypeVariants::Text));
        assert_eq!(val.try_get::<String>(), Some("hello".to_string()));
    }

    #[test]
    fn test_real_round_trip() {
        let val = AnySqliteType::new(3.14f64);
        assert_eq!(val.type_variant(), Some(SqliteTypeVariants::Real));
        assert_eq!(val.try_get::<f64>(), Some(3.14));
    }

    #[test]
    fn test_bool_round_trip() {
        let val = AnySqliteType::new(true);
        assert_eq!(val.type_variant(), Some(SqliteTypeVariants::Bool));
        assert_eq!(val.try_get::<bool>(), Some(true));
        // Bool is a distinct variant — i64 extraction blocked by type boundary
        assert_eq!(val.try_get::<i64>(), None);
    }

    #[test]
    fn test_null_round_trip() {
        let val = AnySqliteType::new(None::<i64>);
        // Value is Null, variant is Integer (from Option<i64>::Target)
        assert_eq!(*val.value(), serde_json::Value::Null);
        assert_eq!(val.type_variant(), Some(SqliteTypeVariants::Integer));

        // from_json on the value directly should work
        let direct = <Option<i64> as SqliteType>::from_json(serde_json::Value::Null);
        assert_eq!(direct, Some(None));

        // try_get: variant matches (Integer == Integer), then from_json(Null) → Some(None)
        assert_eq!(val.try_get::<Option<i64>>(), Some(None));
    }

    #[test]
    fn test_type_mismatch_text_as_integer() {
        let val = AnySqliteType::new("hello".to_string());
        // Text variant should not convert to i64
        assert_eq!(val.try_get::<i64>(), None);
    }

    #[test]
    fn test_type_mismatch_integer_as_text() {
        let val = AnySqliteType::new(42i64);
        // Integer variant should not convert to String
        assert_eq!(val.try_get::<String>(), None);
    }

    #[test]
    fn test_type_mismatch_real_as_integer() {
        let val = AnySqliteType::new(3.14f64);
        // Real variant should not convert to i64
        assert_eq!(val.try_get::<i64>(), None);
    }
}
