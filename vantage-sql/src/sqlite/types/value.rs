//! AnySqliteType extras: From conversions, Display, Expressive.

use super::{AnySqliteType, SqliteType, SqliteTypeNullMarker};
use serde_json::Value;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

impl AnySqliteType {
    /// Create an AnySqliteType with no type marker. Used for values coming
    /// back from the database where we don't know the original type.
    /// `try_get` on these values bypasses variant checking.
    pub fn untyped(value: serde_json::Value) -> Self {
        Self {
            value,
            type_variant: None,
        }
    }
}

/// AnySqliteType is itself a SqliteType — passthrough for type-erased values.
impl SqliteType for AnySqliteType {
    type Target = SqliteTypeNullMarker;

    fn to_json(&self) -> Value {
        self.value().clone()
    }

    fn from_json(value: Value) -> Option<Self> {
        AnySqliteType::from_json(&value)
    }
}

impl std::fmt::Display for AnySqliteType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.value() {
            Value::Null => write!(f, "NULL"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Number(n) => write!(f, "{}", n),
            Value::String(s) => write!(f, "'{}'", s.replace('\'', "''")),
            other => write!(f, "{}", other),
        }
    }
}

// From impls for common types
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

impl_from_for_any_sqlite!(i8, i16, i32, i64, u8, u16, u32, f32, f64, bool, String);

impl From<&str> for AnySqliteType {
    fn from(val: &str) -> Self {
        AnySqliteType::new(val.to_string())
    }
}

// Expressive impls — allows passing scalars directly into sql_expr!
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

impl_expressive_for_sqlite_scalar!(i8, i16, i32, i64, u8, u16, u32, f32, f64, bool, String);

impl Expressive<AnySqliteType> for &str {
    fn expr(&self) -> Expression<AnySqliteType> {
        Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnySqliteType::from(self.to_string()))],
        )
    }
}

// TryFrom<AnySqliteType> impls — enables AssociatedExpression::get()
//
// If the value is a single-row, single-column result (common for COUNT, MAX, etc.),
// extracts the scalar automatically. Otherwise falls back to try_get.
macro_rules! impl_try_from_sqlite {
    ($($ty:ty),*) => {
        $(
            impl TryFrom<AnySqliteType> for $ty {
                type Error = vantage_core::VantageError;
                fn try_from(val: AnySqliteType) -> Result<Self, Self::Error> {
                    // Try direct extraction first
                    if let Some(v) = val.try_get::<$ty>() {
                        return Ok(v);
                    }
                    // If result is [{col: value}], extract the scalar
                    if let serde_json::Value::Array(arr) = val.value() {
                        if arr.len() == 1 {
                            if let Some(obj) = arr[0].as_object() {
                                if obj.len() == 1 {
                                    let inner = AnySqliteType::untyped(
                                        obj.values().next().unwrap().clone()
                                    );
                                    if let Some(v) = inner.try_get::<$ty>() {
                                        return Ok(v);
                                    }
                                }
                            }
                        }
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

/// Extract first row from a result array as a JSON object.
/// Used by TryFrom impls for Record and serde structs.
fn extract_first_row(val: &AnySqliteType) -> Option<serde_json::Value> {
    match val.value() {
        serde_json::Value::Array(arr) => arr.first().cloned(),
        // Already a single object (shouldn't happen from execute, but handle it)
        obj @ serde_json::Value::Object(_) => Some(obj.clone()),
        _ => None,
    }
}

impl TryFrom<AnySqliteType> for vantage_types::Record<serde_json::Value> {
    type Error = vantage_core::VantageError;
    fn try_from(val: AnySqliteType) -> Result<Self, Self::Error> {
        let row = extract_first_row(&val).ok_or_else(|| {
            vantage_core::error!("Expected row result", value = format!("{}", val))
        })?;
        Ok(row.into())
    }
}

impl TryFrom<AnySqliteType> for vantage_types::Record<AnySqliteType> {
    type Error = vantage_core::VantageError;
    fn try_from(val: AnySqliteType) -> Result<Self, Self::Error> {
        let row = extract_first_row(&val).ok_or_else(|| {
            vantage_core::error!("Expected row result", value = format!("{}", val))
        })?;
        // Convert JSON object → Record<AnySqliteType> with untyped values
        match row {
            serde_json::Value::Object(map) => {
                Ok(map.into_iter()
                    .map(|(k, v)| (k, AnySqliteType::untyped(v)))
                    .collect())
            }
            _ => Err(vantage_core::error!("Expected object row", value = format!("{:?}", row))),
        }
    }
}

impl Expressive<AnySqliteType> for AnySqliteType {
    fn expr(&self) -> Expression<AnySqliteType> {
        Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(self.clone())],
        )
    }
}
