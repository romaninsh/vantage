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
            Value::String(s) => write!(f, "'{}'", s),
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

impl Expressive<AnySqliteType> for AnySqliteType {
    fn expr(&self) -> Expression<AnySqliteType> {
        Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(self.clone())],
        )
    }
}
