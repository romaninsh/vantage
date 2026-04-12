//! PostgreSQL Type System
//!
//! Uses `ciborium::Value` as the underlying value type for lossless storage.
//! CBOR tags follow SurrealDB conventions where they overlap:
//!   Tag(0)   = DateTime (RFC 3339)
//!   Tag(10)  = Decimal (string representation)
//!   Tag(100) = Date (YYYY-MM-DD string)
//!   Tag(101) = Time (HH:MM:SS string)
//!   Tag(9)   = UUID (string representation)
//!
//! Type variants track the original PostgreSQL column type so that bind and
//! entity deserialization can reconstruct the correct Rust type.

use vantage_types::vantage_type_system;

vantage_type_system! {
    type_trait: PostgresType,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [
        Null,
        Bool,       // BOOLEAN
        Int2,       // SMALLINT, INT2
        Int4,       // INTEGER, INT, INT4, SERIAL
        Int8,       // BIGINT, INT8, BIGSERIAL
        Float4,     // REAL, FLOAT4
        Float8,     // DOUBLE PRECISION, FLOAT8
        Text,       // TEXT, VARCHAR, CHAR, NAME
        Decimal,    // NUMERIC, DECIMAL
        DateTime,   // TIMESTAMPTZ, TIMESTAMP
        Date,       // DATE
        Time,       // TIME
        Uuid,       // UUID
        Blob        // BYTEA
    ]
}

impl PostgresTypeVariants {
    /// Detect the type variant from a CBOR value.
    pub fn from_cbor(value: &ciborium::Value) -> Option<Self> {
        use ciborium::Value::*;

        match value {
            Null => Some(Self::Null),
            Bool(_) => Some(Self::Bool),
            Integer(_) => Some(Self::Int8),
            Float(_) => Some(Self::Float8),
            Text(_) => Some(Self::Text),
            Bytes(_) => Some(Self::Blob),
            Tag(0, _) => Some(Self::DateTime),
            Tag(9, _) => Some(Self::Uuid),
            Tag(10, _) => Some(Self::Decimal),
            Tag(100, _) => Some(Self::Date),
            Tag(101, _) => Some(Self::Time),
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
        assert_eq!(val.try_get::<i64>(), None);
    }

    #[test]
    fn test_null_round_trip() {
        let val = AnyPostgresType::new(None::<i64>);
        assert_eq!(*val.value(), ciborium::Value::Null);
        assert_eq!(val.type_variant(), Some(PostgresTypeVariants::Int8));

        let direct = <Option<i64> as PostgresType>::from_cbor(ciborium::Value::Null);
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

    #[test]
    fn test_untyped_integer_try_get() {
        let val = AnyPostgresType::untyped(ciborium::Value::Integer(42.into()));
        assert_eq!(val.try_get::<i64>(), Some(42));
        assert_eq!(val.try_get::<i32>(), Some(42));
    }

    #[test]
    fn test_untyped_text_try_get() {
        let val = AnyPostgresType::untyped(ciborium::Value::Text("world".into()));
        assert_eq!(val.try_get::<String>(), Some("world".to_string()));
    }

    #[test]
    fn test_untyped_decimal_tag() {
        let val = AnyPostgresType::untyped(ciborium::Value::Tag(
            10,
            Box::new(ciborium::Value::Text("123.456".into())),
        ));
        assert_eq!(
            PostgresTypeVariants::from_cbor(val.value()),
            Some(PostgresTypeVariants::Decimal)
        );
    }

    #[test]
    fn test_untyped_datetime_tag() {
        let val = AnyPostgresType::untyped(ciborium::Value::Tag(
            0,
            Box::new(ciborium::Value::Text("2024-01-15T10:30:00".into())),
        ));
        assert_eq!(
            PostgresTypeVariants::from_cbor(val.value()),
            Some(PostgresTypeVariants::DateTime)
        );
    }

    #[test]
    fn test_untyped_uuid_tag() {
        let val = AnyPostgresType::untyped(ciborium::Value::Tag(
            9,
            Box::new(ciborium::Value::Text(
                "550e8400-e29b-41d4-a716-446655440000".into(),
            )),
        ));
        assert_eq!(
            PostgresTypeVariants::from_cbor(val.value()),
            Some(PostgresTypeVariants::Uuid)
        );
    }
}
