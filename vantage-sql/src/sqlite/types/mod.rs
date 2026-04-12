//! SQLite Type System
//!
//! Uses `ciborium::Value` as the underlying value type for lossless storage.
//! CBOR tags follow the same conventions as MySQL/Postgres backends:
//!   Tag(0)   = DateTime (RFC 3339)
//!   Tag(10)  = Decimal/Numeric (string representation)
//!   Tag(100) = Date (YYYY-MM-DD string)
//!   Tag(101) = Time (HH:MM:SS string)
//!
//! SQLite type variants map to SQLite's affinity system:
//! - INTEGER affinity: INTEGER, INT, TINYINT, SMALLINT, MEDIUMINT, BIGINT
//! - TEXT    affinity: TEXT, VARCHAR(N), CHAR(N), CLOB, NVARCHAR(N)
//! - REAL    affinity: REAL, FLOAT, DOUBLE, DOUBLE PRECISION
//! - NUMERIC affinity: NUMERIC, DECIMAL(P,S), BOOLEAN, DATE, DATETIME
//! - BLOB    affinity: BLOB

use vantage_types::vantage_type_system;

vantage_type_system! {
    type_trait: SqliteType,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [
        Null,
        Bool,       // BOOLEAN — stored as 0/1 on disk, distinct from Integer
        Integer,    // i8..i64, u8..u32
        Text,       // String, &str
        Real,       // f32, f64
        Numeric,    // DECIMAL — Tag(10, Text("..."))
        Blob        // Vec<u8> — CborValue::Bytes
    ]
}

impl SqliteTypeVariants {
    /// Detect the type variant from a CBOR value.
    pub fn from_cbor(value: &ciborium::Value) -> Option<Self> {
        use ciborium::Value::*;

        match value {
            Null => Some(Self::Null),
            Bool(_) => Some(Self::Bool),
            Integer(_) => Some(Self::Integer),
            Float(_) => Some(Self::Real),
            Text(_) => Some(Self::Text),
            Bytes(_) => Some(Self::Blob),
            Tag(10, _) => Some(Self::Numeric),
            Tag(0 | 100 | 101, _) => Some(Self::Text),
            Tag(_, inner) => Self::from_cbor(inner),
            _ => None,
        }
    }
}

mod bool;
mod chrono;
mod decimal;
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
        let val = AnySqliteType::new(3.15f64);
        assert_eq!(val.type_variant(), Some(SqliteTypeVariants::Real));
        assert_eq!(val.try_get::<f64>(), Some(3.15));
    }

    #[test]
    fn test_bool_round_trip() {
        let val = AnySqliteType::new(true);
        assert_eq!(val.type_variant(), Some(SqliteTypeVariants::Bool));
        assert_eq!(val.try_get::<bool>(), Some(true));
        assert_eq!(val.try_get::<i64>(), None);
    }

    #[test]
    fn test_null_round_trip() {
        let val = AnySqliteType::new(None::<i64>);
        assert_eq!(*val.value(), ciborium::Value::Null);
        assert_eq!(val.type_variant(), Some(SqliteTypeVariants::Integer));

        let direct = <Option<i64> as SqliteType>::from_cbor(ciborium::Value::Null);
        assert_eq!(direct, Some(None));

        assert_eq!(val.try_get::<Option<i64>>(), Some(None));
    }

    #[test]
    fn test_type_mismatch_text_as_integer() {
        let val = AnySqliteType::new("hello".to_string());
        assert_eq!(val.try_get::<i64>(), None);
    }

    #[test]
    fn test_type_mismatch_integer_as_text() {
        let val = AnySqliteType::new(42i64);
        assert_eq!(val.try_get::<String>(), None);
    }

    #[test]
    fn test_type_mismatch_real_as_integer() {
        let val = AnySqliteType::new(3.15f64);
        assert_eq!(val.try_get::<i64>(), None);
    }

    #[test]
    fn test_untyped_integer_try_get() {
        let val = AnySqliteType::untyped(ciborium::Value::Integer(42.into()));
        assert_eq!(val.try_get::<i64>(), Some(42));
        assert_eq!(val.try_get::<i32>(), Some(42));
    }

    #[test]
    fn test_untyped_text_try_get() {
        let val = AnySqliteType::untyped(ciborium::Value::Text("world".into()));
        assert_eq!(val.try_get::<String>(), Some("world".to_string()));
    }

    #[test]
    fn test_untyped_numeric_tag() {
        let val = AnySqliteType::untyped(ciborium::Value::Tag(
            10,
            Box::new(ciborium::Value::Text("123.456".into())),
        ));
        assert_eq!(
            SqliteTypeVariants::from_cbor(val.value()),
            Some(SqliteTypeVariants::Numeric)
        );
    }
}
