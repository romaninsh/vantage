//! AnyMysqlType extras: From conversions, Display, Expressive.

use super::{AnyMysqlType, MysqlType, MysqlTypeNullMarker};
use serde_json::Value;
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

impl AnyMysqlType {
    /// Create an AnyMysqlType with no type marker. Used for values coming
    /// back from the database where we don't know the original type.
    /// `try_get` on these values bypasses variant checking.
    pub fn untyped(value: serde_json::Value) -> Self {
        Self {
            value,
            type_variant: None,
        }
    }
}

/// AnyMysqlType is itself a MysqlType -- passthrough for type-erased values.
impl MysqlType for AnyMysqlType {
    type Target = MysqlTypeNullMarker;

    fn to_json(&self) -> Value {
        self.value().clone()
    }

    fn from_json(value: Value) -> Option<Self> {
        AnyMysqlType::from_json(&value)
    }
}

impl std::fmt::Display for AnyMysqlType {
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
macro_rules! impl_from_for_any_mysql {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for AnyMysqlType {
                fn from(val: $ty) -> Self {
                    AnyMysqlType::new(val)
                }
            }
        )*
    };
}

impl_from_for_any_mysql!(i8, i16, i32, i64, u8, u16, u32, f32, f64, bool, String);

impl From<&str> for AnyMysqlType {
    fn from(val: &str) -> Self {
        AnyMysqlType::new(val.to_string())
    }
}

// Expressive impls -- allows passing scalars directly into mysql_expr!
macro_rules! impl_expressive_for_mysql_scalar {
    ($($ty:ty),*) => {
        $(
            impl Expressive<AnyMysqlType> for $ty {
                fn expr(&self) -> Expression<AnyMysqlType> {
                    Expression::new(
                        "{}",
                        vec![ExpressiveEnum::Scalar(AnyMysqlType::new_ref(self))],
                    )
                }
            }
        )*
    };
}

impl_expressive_for_mysql_scalar!(i8, i16, i32, i64, u8, u16, u32, f32, f64, bool, String);

impl Expressive<AnyMysqlType> for &str {
    fn expr(&self) -> Expression<AnyMysqlType> {
        Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnyMysqlType::from(
                self.to_string(),
            ))],
        )
    }
}

// TryFrom<AnyMysqlType> impls -- enables AssociatedExpression::get()
macro_rules! impl_try_from_mysql {
    ($($ty:ty),*) => {
        $(
            impl TryFrom<AnyMysqlType> for $ty {
                type Error = vantage_core::VantageError;
                fn try_from(val: AnyMysqlType) -> Result<Self, Self::Error> {
                    // Try direct extraction first
                    if let Some(v) = val.try_get::<$ty>() {
                        return Ok(v);
                    }
                    // If result is [{col: value}], extract the scalar
                    if let serde_json::Value::Array(arr) = val.value() {
                        if arr.len() == 1 {
                            if let Some(obj) = arr[0].as_object() {
                                if obj.len() == 1 {
                                    let inner = AnyMysqlType::untyped(
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
                        "Cannot convert AnyMysqlType to target type",
                        target = std::any::type_name::<$ty>(),
                        value = format!("{}", val)
                    ))
                }
            }
        )*
    };
}

impl_try_from_mysql!(i64, i32, f64, bool, String);

/// Extract first row from a result array as a JSON object.
fn extract_first_row(val: &AnyMysqlType) -> Option<serde_json::Value> {
    match val.value() {
        serde_json::Value::Array(arr) => arr.first().cloned(),
        obj @ serde_json::Value::Object(_) => Some(obj.clone()),
        _ => None,
    }
}

impl TryFrom<AnyMysqlType> for vantage_types::Record<serde_json::Value> {
    type Error = vantage_core::VantageError;
    fn try_from(val: AnyMysqlType) -> Result<Self, Self::Error> {
        let row = extract_first_row(&val).ok_or_else(|| {
            vantage_core::error!("Expected row result", value = format!("{}", val))
        })?;
        Ok(row.into())
    }
}

impl TryFrom<AnyMysqlType> for vantage_types::Record<AnyMysqlType> {
    type Error = vantage_core::VantageError;
    fn try_from(val: AnyMysqlType) -> Result<Self, Self::Error> {
        let row = extract_first_row(&val).ok_or_else(|| {
            vantage_core::error!("Expected row result", value = format!("{}", val))
        })?;
        match row {
            serde_json::Value::Object(map) => Ok(map
                .into_iter()
                .map(|(k, v)| (k, AnyMysqlType::untyped(v)))
                .collect()),
            _ => Err(vantage_core::error!(
                "Expected object row",
                value = format!("{:?}", row)
            )),
        }
    }
}

impl Expressive<AnyMysqlType> for AnyMysqlType {
    fn expr(&self) -> Expression<AnyMysqlType> {
        Expression::new("{}", vec![ExpressiveEnum::Scalar(self.clone())])
    }
}
