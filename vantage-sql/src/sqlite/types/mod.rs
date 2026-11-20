//! SQLite Type System
//!
//! Uses `ciborium::Value` as the underlying value type for lossless storage.
//! CBOR tags:
//!   Tag(0)    = DateTime, Tag(1004) = Date, Tag(101) = Time, in the carrier forms of
//!               `vantage_types::temporal` (Tag(100) dates are still read)
//!   Tag(10)   = Decimal/Numeric (string representation)
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
    null_when: ciborium::Value::Null,
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
            Tag(0 | 100 | 101 | 1004, _) => Some(Self::Text),
            Tag(_, inner) => Self::from_cbor(inner),
            _ => None,
        }
    }
}

mod bool;
mod bytes;
mod chrono;
mod decimal;
mod json;
mod numbers;
mod string;
mod value;
