//! AnySqliteType extras: From conversions, Display, Expressive, TryFrom.
//!
//! Uses ciborium::Value (CBOR) as the underlying value type.

use super::{AnySqliteType, SqliteType, SqliteTypeNullMarker, SqliteTypeVariants};
use ciborium::Value as CborValue;
use serde_json::Value as JsonValue;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

use crate::types::{cbor_to_json, json_to_cbor};

impl AnySqliteType {
    /// Create an AnySqliteType with no type marker. Used for values coming
    /// back from the database where we don't know the original type.
    /// `try_get` on these values bypasses variant checking.
    pub fn untyped(value: CborValue) -> Self {
        Self {
            value,
            type_variant: None,
        }
    }

    /// Create an AnySqliteType with an explicit type variant.
    /// Used by row_to_record where the SQLite column type is known.
    pub fn with_variant(value: CborValue, variant: SqliteTypeVariants) -> Self {
        Self {
            value,
            type_variant: Some(variant),
        }
    }

    /// If this is a single-row, single-column result `[{col: value}]`,
    /// extract the scalar value. Otherwise return self unchanged.
    pub fn unwrap_scalar(self) -> Self {
        match self.value() {
            CborValue::Array(arr) if arr.len() == 1 => {
                if let CborValue::Map(map) = &arr[0]
                    && map.len() == 1
                {
                    let (_, v) = &map[0];
                    return AnySqliteType::untyped(v.clone());
                }
                self
            }
            _ => self,
        }
    }
}

/// AnySqliteType is itself a SqliteType — passthrough for type-erased values.
impl SqliteType for AnySqliteType {
    type Target = SqliteTypeNullMarker;

    fn to_cbor(&self) -> CborValue {
        self.value().clone()
    }

    fn from_cbor(value: CborValue) -> Option<Self> {
        AnySqliteType::from_cbor(&value)
    }
}

impl std::fmt::Display for AnySqliteType {
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
            CborValue::Bytes(b) => write!(f, "x'{}'", hex::encode(b)),
            CborValue::Tag(10, inner) => {
                if let CborValue::Text(s) = inner.as_ref() {
                    write!(f, "{}", s)
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
                    if let Some(any) = AnySqliteType::from_cbor(item) {
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
                    if let Some(any) = AnySqliteType::from_cbor(v) {
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

macro_rules! impl_from_for_any_sqlite {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for AnySqliteType {
                fn from(val: $ty) -> Self {
                    AnySqliteType::new(val)
                }
            }
        )*
    };
}

impl_from_for_any_sqlite!(
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
    chrono::DateTime<chrono::Utc>
);

impl From<&str> for AnySqliteType {
    fn from(val: &str) -> Self {
        AnySqliteType::new(val.to_string())
    }
}

// -- JSON <-> CBOR bridge (for AnyTable interop) ------------------------------

impl From<JsonValue> for AnySqliteType {
    fn from(val: JsonValue) -> Self {
        if val.is_null() {
            return AnySqliteType::untyped(CborValue::Null);
        }
        let cbor = json_to_cbor(val);
        AnySqliteType::from_cbor(&cbor).expect("json_to_cbor produced unconvertible CBOR")
    }
}

impl From<AnySqliteType> for JsonValue {
    fn from(val: AnySqliteType) -> Self {
        cbor_to_json(val.into_value())
    }
}

// -- Expressive impls ---------------------------------------------------------

macro_rules! impl_expressive_for_sqlite_scalar {
    ($($ty:ty),*) => {
        $(
            impl Expressive<AnySqliteType> for $ty {
                fn expr(&self) -> Expression<AnySqliteType> {
                    Expression::new(
                        "{}",
                        vec![ExpressiveEnum::Scalar(AnySqliteType::new_ref(self))],
                    )
                }
            }
        )*
    };
}

impl_expressive_for_sqlite_scalar!(
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
    chrono::DateTime<chrono::Utc>
);

impl Expressive<AnySqliteType> for &str {
    fn expr(&self) -> Expression<AnySqliteType> {
        Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnySqliteType::from(
                self.to_string(),
            ))],
        )
    }
}

impl Expressive<AnySqliteType> for AnySqliteType {
    fn expr(&self) -> Expression<AnySqliteType> {
        Expression::new("{}", vec![ExpressiveEnum::Scalar(self.clone())])
    }
}

// -- TryFrom impls (for AssociatedExpression::get()) --------------------------

macro_rules! impl_try_from_sqlite {
    ($($ty:ty),*) => {
        $(
            impl TryFrom<AnySqliteType> for $ty {
                type Error = vantage_core::VantageError;
                fn try_from(val: AnySqliteType) -> Result<Self, Self::Error> {
                    if let Some(v) = val.try_get::<$ty>() {
                        return Ok(v);
                    }
                    // If result is [{col: value}], extract the scalar
                    let val = val.unwrap_scalar();
                    if let Some(v) = val.try_get::<$ty>() {
                        return Ok(v);
                    }
                    Err(vantage_core::error!(
                        "Cannot convert AnySqliteType to target type",
                        target = std::any::type_name::<$ty>(),
                        value = format!("{}", val)
                    ))
                }
            }
        )*
    };
}

impl_try_from_sqlite!(i64, i32, f64, bool, String);

/// Extract first row from a CBOR result array as a map.
fn extract_first_row(val: &AnySqliteType) -> Option<CborValue> {
    match val.value() {
        CborValue::Array(arr) => arr.first().cloned(),
        obj @ CborValue::Map(_) => Some(obj.clone()),
        _ => None,
    }
}

/// Convert a CBOR Map into a Record<AnySqliteType>.
fn cbor_map_to_record(map: Vec<(CborValue, CborValue)>) -> vantage_types::Record<AnySqliteType> {
    map.into_iter()
        .map(|(k, v)| {
            let key = match k {
                CborValue::Text(s) => s,
                other => format!("{:?}", other),
            };
            (key, AnySqliteType::untyped(v))
        })
        .collect()
}

impl TryFrom<AnySqliteType> for vantage_types::Record<AnySqliteType> {
    type Error = vantage_core::VantageError;
    fn try_from(val: AnySqliteType) -> Result<Self, Self::Error> {
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

impl TryFrom<AnySqliteType> for Vec<vantage_types::Record<AnySqliteType>> {
    type Error = vantage_core::VantageError;
    fn try_from(val: AnySqliteType) -> Result<Self, Self::Error> {
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

impl TryFrom<AnySqliteType> for vantage_types::Record<serde_json::Value> {
    type Error = vantage_core::VantageError;
    fn try_from(val: AnySqliteType) -> Result<Self, Self::Error> {
        let record: vantage_types::Record<AnySqliteType> = val.try_into()?;
        Ok(record
            .into_iter()
            .map(|(k, v)| (k, cbor_to_json(v.into_value())))
            .collect())
    }
}

impl vantage_types::TerminalRender for AnySqliteType {
    fn render(&self) -> String {
        match self.value() {
            CborValue::Null => "-".to_string(),
            CborValue::Text(s) => s.clone(),
            CborValue::Bool(b) => b.to_string(),
            CborValue::Tag(10, inner) => {
                if let CborValue::Text(s) = inner.as_ref() {
                    s.clone()
                } else {
                    format!("{}", self)
                }
            }
            CborValue::Tag(0, inner) | CborValue::Tag(100, inner) | CborValue::Tag(101, inner) => {
                if let CborValue::Text(s) = inner.as_ref() {
                    s.clone()
                } else {
                    format!("{}", self)
                }
            }
            _ => format!("{}", self),
        }
    }

    fn color_hint(&self) -> Option<&'static str> {
        match self.value() {
            CborValue::Bool(true) => Some("green"),
            CborValue::Bool(false) => Some("red"),
            CborValue::Null => Some("dim"),
            _ => None,
        }
    }
}
