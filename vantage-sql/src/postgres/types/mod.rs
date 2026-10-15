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
    null_when: ciborium::Value::Null,
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
