//! AnyPostgresType extras: From conversions, Display, Expressive, TryFrom.
//!
//! Uses ciborium::Value (CBOR) as the underlying value type.

use super::{AnyPostgresType, PostgresType, PostgresTypeNullMarker, PostgresTypeVariants};
use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::types::{cbor_to_json, json_to_cbor};

impl AnyPostgresType {
    /// Create an AnyPostgresType with no type marker. Used for values coming
    /// back from the database where we don't know the original type.
    /// `try_get` on these values bypasses variant checking.
    pub fn untyped(value: CborValue) -> Self {
        Self {
            value,
            type_variant: None,
        }
    }

    /// Create an AnyPostgresType with an explicit type variant.
    /// Used by row_to_record where the PostgreSQL column type is known.
    pub fn with_variant(value: CborValue, variant: PostgresTypeVariants) -> Self {
        Self {
            value,
            type_variant: Some(variant),
        }
    }
}

/// AnyPostgresType is itself a PostgresType -- passthrough for type-erased values.
impl PostgresType for AnyPostgresType {
    type Target = PostgresTypeNullMarker;

    fn to_cbor(&self) -> CborValue {
        self.value().clone()
    }

    fn from_cbor(value: CborValue) -> Option<Self> {
        AnyPostgresType::from_cbor(&value)
    }
}

impl std::fmt::Display for AnyPostgresType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.value() {
            CborValue::Null => write!(f, "NULL"),
            CborValue::Bool(b) => write!(f, "{}", b),
            CborValue::Integer(i) => write!(f, "{}", i128::from(*i)),
            CborValue::Float(v) => {
                if v.fract() == 0.0 {
                    write!(f, "{:.1}", v)
                } else {
                    write!(f, "{}", v)
                }
            }
            CborValue::Text(s) => write!(f, "'{}'", s.replace('\'', "''")),
            CborValue::Bytes(b) => write!(f, "\\x{}", hex::encode(b)),
            CborValue::Tag(10, inner) => {
                if let CborValue::Text(s) = inner.as_ref() {
                    write!(f, "{}", s)
                } else {
                    write!(f, "{:?}", inner)
                }
            }
            CborValue::Tag(9, inner) => {
                // UUID
                if let CborValue::Text(s) = inner.as_ref() {
                    write!(f, "'{}'", s)
                } else {
                    write!(f, "{:?}", inner)
                }
            }
            CborValue::Tag(0, inner) | CborValue::Tag(100, inner) | CborValue::Tag(101, inner) => {
                if let CborValue::Text(s) = inner.as_ref() {
                    write!(f, "'{}'", s)
                } else {
                    write!(f, "{:?}", inner)
                }
            }
            CborValue::Array(arr) => {
                write!(f, "[")?;
                for (i, item) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    if let Some(any) = AnyPostgresType::from_cbor(item) {
                        write!(f, "{}", any)?;
                    } else {
                        write!(f, "{:?}", item)?;
                    }
                }
                write!(f, "]")
            }
            CborValue::Map(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    let key_str = match k {
                        CborValue::Text(s) => s.clone(),
                        _ => format!("{:?}", k),
                    };
                    if let Some(any) = AnyPostgresType::from_cbor(v) {
                        write!(f, "{}: {}", key_str, any)?;
                    } else {
                        write!(f, "{}: {:?}", key_str, v)?;
                    }
                }
                write!(f, "}}")
            }
            other => write!(f, "{:?}", other),
        }
    }
}

// -- From impls for common types ----------------------------------------------

macro_rules! impl_from_for_any_postgres {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for AnyPostgresType {
                fn from(val: $ty) -> Self {
                    AnyPostgresType::new(val)
                }
            }
        )*
    };
}

impl_from_for_any_postgres!(
    i8,
    i16,
    i32,
    i64,
    u8,
    u16,
    u32,
    f32,
    f64,
    bool,
    String,
    chrono::NaiveDate,
    chrono::NaiveTime,
    chrono::NaiveDateTime,
    chrono::DateTime<chrono::Utc>,
    rust_decimal::Decimal
);

impl From<&str> for AnyPostgresType {
    fn from(val: &str) -> Self {
        AnyPostgresType::new(val.to_string())
    }
}

// -- JSON <-> CBOR bridge (for AnyTable interop) ------------------------------

impl From<JsonValue> for AnyPostgresType {
    fn from(val: JsonValue) -> Self {
        if val.is_null() {
            return AnyPostgresType::untyped(CborValue::Null);
        }
        let cbor = json_to_cbor(val);
        AnyPostgresType::from_cbor(&cbor).expect("json_to_cbor produced unconvertible CBOR")
    }
}

impl From<AnyPostgresType> for JsonValue {
    fn from(val: AnyPostgresType) -> Self {
        cbor_to_json(val.into_value())
    }
}

// -- Expressive impls ---------------------------------------------------------

macro_rules! impl_expressive_for_postgres_scalar {
    ($($ty:ty),*) => {
        $(
            impl Expressive<AnyPostgresType> for $ty {
                fn expr(&self) -> Expression<AnyPostgresType> {
                    Expression::new(
                        "{}",
                        vec![ExpressiveEnum::Scalar(AnyPostgresType::new_ref(self))],
                    )
                }
            }
        )*
    };
}

impl_expressive_for_postgres_scalar!(
    i8,
    i16,
    i32,
    i64,
    u8,
    u16,
    u32,
    f32,
    f64,
    bool,
    String,
    chrono::NaiveDate,
    chrono::NaiveTime,
    chrono::NaiveDateTime,
    chrono::DateTime<chrono::Utc>,
    rust_decimal::Decimal
);

impl Expressive<AnyPostgresType> for &str {
    fn expr(&self) -> Expression<AnyPostgresType> {
        Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnyPostgresType::from(
                self.to_string(),
            ))],
        )
    }
}

impl Expressive<AnyPostgresType> for AnyPostgresType {
    fn expr(&self) -> Expression<AnyPostgresType> {
        Expression::new("{}", vec![ExpressiveEnum::Scalar(self.clone())])
    }
}

// -- TryFrom impls (for AssociatedExpression::get()) --------------------------

macro_rules! impl_try_from_postgres {
    ($($ty:ty),*) => {
        $(
            impl TryFrom<AnyPostgresType> for $ty {
                type Error = vantage_core::VantageError;
                fn try_from(val: AnyPostgresType) -> Result<Self, Self::Error> {
                    if let Some(v) = val.try_get::<$ty>() {
                        return Ok(v);
                    }
                    // If result is [{col: value}], extract the scalar
                    if let CborValue::Array(ref arr) = *val.value() {
                        if arr.len() == 1 {
                            if let CborValue::Map(ref map) = arr[0] {
                                if map.len() == 1 {
                                    let (_, v) = &map[0];
                                    let inner = AnyPostgresType::untyped(v.clone());
                                    if let Some(v) = inner.try_get::<$ty>() {
                                        return Ok(v);
                                    }
                                }
                            }
                        }
                    }
                    Err(vantage_core::error!(
                        "Cannot convert AnyPostgresType to target type",
                        target = std::any::type_name::<$ty>(),
                        value = format!("{}", val)
                    ))
                }
            }
        )*
    };
}

impl_try_from_postgres!(i64, i32, f64, bool, String, rust_decimal::Decimal);

/// Extract first row from a CBOR result array as a map.
fn extract_first_row(val: &AnyPostgresType) -> Option<CborValue> {
    match val.value() {
        CborValue::Array(arr) => arr.first().cloned(),
        obj @ CborValue::Map(_) => Some(obj.clone()),
        _ => None,
    }
}

/// Convert a CBOR Map into a Record<AnyPostgresType>.
fn cbor_map_to_record(map: Vec<(CborValue, CborValue)>) -> vantage_types::Record<AnyPostgresType> {
    map.into_iter()
        .map(|(k, v)| {
            let key = match k {
                CborValue::Text(s) => s,
                other => format!("{:?}", other),
            };
            (key, AnyPostgresType::untyped(v))
        })
        .collect()
}

impl TryFrom<AnyPostgresType> for vantage_types::Record<AnyPostgresType> {
    type Error = vantage_core::VantageError;
    fn try_from(val: AnyPostgresType) -> Result<Self, Self::Error> {
        let row = extract_first_row(&val).ok_or_else(|| {
            vantage_core::error!("Expected row result", value = format!("{}", val))
        })?;
        match row {
            CborValue::Map(map) => Ok(cbor_map_to_record(map)),
            _ => Err(vantage_core::error!(
                "Expected map row",
                value = format!("{:?}", row)
            )),
        }
    }
}

impl TryFrom<AnyPostgresType> for Vec<vantage_types::Record<AnyPostgresType>> {
    type Error = vantage_core::VantageError;
    fn try_from(val: AnyPostgresType) -> Result<Self, Self::Error> {
        match val.into_value() {
            CborValue::Array(arr) => arr
                .into_iter()
                .map(|item| match item {
                    CborValue::Map(map) => Ok(cbor_map_to_record(map)),
                    other => Err(vantage_core::error!(
                        "Expected map row",
                        value = format!("{:?}", other)
                    )),
                })
                .collect(),
            CborValue::Map(map) => Ok(vec![cbor_map_to_record(map)]),
            other => Err(vantage_core::error!(
                "Expected array or map result",
                value = format!("{:?}", other)
            )),
        }
    }
}

impl TryFrom<AnyPostgresType> for vantage_types::Record<serde_json::Value> {
    type Error = vantage_core::VantageError;
    fn try_from(val: AnyPostgresType) -> Result<Self, Self::Error> {
        let record: vantage_types::Record<AnyPostgresType> = val.try_into()?;
        Ok(record
            .into_iter()
            .map(|(k, v)| (k, cbor_to_json(v.into_value())))
            .collect())
    }
}

impl vantage_types::TerminalRender for AnyPostgresType {
    fn render(&self) -> vantage_types::RichText {
        use vantage_types::{RichText, Style};
        match self.value() {
            CborValue::Null => RichText::styled("—", Style::Muted),
            CborValue::Text(s) => RichText::plain(s.clone()),
            CborValue::Bool(true) => RichText::styled("true", Style::Success),
            CborValue::Bool(false) => RichText::styled("false", Style::Error),
            CborValue::Tag(10, inner)
            | CborValue::Tag(0, inner)
            | CborValue::Tag(100, inner)
            | CborValue::Tag(101, inner)
            | CborValue::Tag(9, inner) => {
                if let CborValue::Text(s) = inner.as_ref() {
                    RichText::plain(s.clone())
                } else {
                    RichText::plain(format!("{}", self))
                }
            }
            _ => RichText::plain(format!("{}", self)),
        }
    }
}
