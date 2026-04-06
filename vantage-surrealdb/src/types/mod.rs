//! SurrealDB Type System for Vantage Framework
//!
//! This module provides a SurrealDB-specific type system using the vantage-types framework.
//! It defines the core SurrealType trait and AnySurrealType for type-erased operations.

use vantage_core::VantageError;
use vantage_types::{Record, TerminalRender, vantage_type_system};

// Generate the SurrealDB type system
vantage_type_system! {
    type_trait: SurrealType,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [
        // Primitive types
        None,
        Bool,
        Int,
        Float,
        String,
        Bytes,

        // Advanced numeric types
        Decimal,
        Number,

        // Temporal types
        DateTime,
        Duration,

        // Identifier types
        Uuid,
        Thing,

        // Collection types
        Array,
        Object,
        Set,

        // Spatial types
        Geometry,
        Point,
        Line,
        Polygon,
        MultiPoint,
        MultiLine,
        MultiPolygon,
        Collection,

        // Range types
        Range
    ]
}

// Override the macro-generated variant detection with SurrealDB-specific logic
impl SurrealTypeVariants {
    /// Detect the SurrealDB type from a CBOR value
    pub fn from_cbor(value: &ciborium::Value) -> Option<Self> {
        use ciborium::Value::*;

        match value {
            Null => Some(SurrealTypeVariants::None),
            Bool(_) => Some(SurrealTypeVariants::Bool),
            Integer(_) => Some(SurrealTypeVariants::Int),
            Float(_) => Some(SurrealTypeVariants::Float),
            Text(_) => Some(SurrealTypeVariants::String),
            Bytes(_) => Some(SurrealTypeVariants::Bytes),
            Array(_) => Some(SurrealTypeVariants::Array),
            Map(_) => Some(SurrealTypeVariants::Object),

            // Tagged values for SurrealDB specific types
            Tag(0, _) => Some(SurrealTypeVariants::DateTime), // RFC 3339 datetime
            Tag(6, _) => Some(SurrealTypeVariants::None),     // NONE value
            Tag(8, _) => Some(SurrealTypeVariants::Thing),    // Record ID
            Tag(9, _) => Some(SurrealTypeVariants::Uuid),     // UUID string
            Tag(10, _) => Some(SurrealTypeVariants::Decimal), // Decimal string
            Tag(12, _) => Some(SurrealTypeVariants::DateTime), // Compact datetime
            Tag(13, _) => Some(SurrealTypeVariants::Duration), // Duration string
            Tag(14, _) => Some(SurrealTypeVariants::Duration), // Compact duration
            Tag(37, _) => Some(SurrealTypeVariants::Uuid),    // UUID binary
            Tag(49, _) => Some(SurrealTypeVariants::Range),   // Range
            Tag(88, _) => Some(SurrealTypeVariants::Point),   // Geometry Point
            Tag(89, _) => Some(SurrealTypeVariants::Line),    // Geometry Line
            Tag(90, _) => Some(SurrealTypeVariants::Polygon), // Geometry Polygon
            Tag(91, _) => Some(SurrealTypeVariants::MultiPoint), // Geometry MultiPoint
            Tag(92, _) => Some(SurrealTypeVariants::MultiLine), // Geometry MultiLine
            Tag(93, _) => Some(SurrealTypeVariants::MultiPolygon), // Geometry MultiPolygon
            Tag(94, _) => Some(SurrealTypeVariants::Collection), // Geometry Collection

            // Recurse into other tagged values
            Tag(_, boxed_value) => Self::from_cbor(boxed_value),

            // Unknown/unhandled types
            _ => None,
        }
    }

    /// Check if this type is a primitive type
    pub fn is_primitive(&self) -> bool {
        matches!(
            self,
            SurrealTypeVariants::None
                | SurrealTypeVariants::Bool
                | SurrealTypeVariants::Int
                | SurrealTypeVariants::Float
                | SurrealTypeVariants::String
                | SurrealTypeVariants::Bytes
        )
    }

    /// Check if this type is a collection type
    pub fn is_collection(&self) -> bool {
        matches!(
            self,
            SurrealTypeVariants::Array | SurrealTypeVariants::Object | SurrealTypeVariants::Set
        )
    }

    /// Check if this type is a geometry type
    pub fn is_geometry(&self) -> bool {
        matches!(
            self,
            SurrealTypeVariants::Geometry
                | SurrealTypeVariants::Point
                | SurrealTypeVariants::Line
                | SurrealTypeVariants::Polygon
                | SurrealTypeVariants::MultiPoint
                | SurrealTypeVariants::MultiLine
                | SurrealTypeVariants::MultiPolygon
                | SurrealTypeVariants::Collection
        )
    }

    /// Check if this type is a temporal type
    pub fn is_temporal(&self) -> bool {
        matches!(
            self,
            SurrealTypeVariants::DateTime | SurrealTypeVariants::Duration
        )
    }
}

// Type implementations are organized in separate modules
mod bool;
mod decimal;
mod generic;
mod numbers;
mod string;
mod value;

// Re-export the implementations
// pub use bool::*;
// pub use decimal::*;
// pub use numbers::*;
// pub use string::*;

// // Blanket From implementation for any type that implements SurrealType
// impl<T> From<T> for AnySurrealType
// where
//     T: SurrealType,
// {
//     fn from(value: T) -> Self {
//         AnySurrealType::new(value)
//     }
// }

impl std::fmt::Display for AnySurrealType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        use ciborium::Value;
        match &self.value {
            Value::Null => write!(f, "NULL"),
            Value::Bool(b) => write!(f, "{}", b),
            Value::Integer(i) => write!(f, "{}", i128::from(*i)),
            Value::Float(v) => write!(f, "{}", v),
            Value::Text(s) => write!(f, "\"{}\"", s),
            Value::Bytes(b) => write!(f, "h'{}'", hex::encode(b)),
            Value::Array(arr) => {
                write!(f, "[")?;
                for (i, item) in arr.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    if let Some(any) = AnySurrealType::from_cbor(item) {
                        write!(f, "{}", any)?;
                    } else {
                        write!(f, "{:?}", item)?;
                    }
                }
                write!(f, "]")
            }
            Value::Map(map) => {
                write!(f, "{{")?;
                for (i, (k, v)) in map.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    let key_str = match k {
                        Value::Text(s) => s.clone(),
                        _ => format!("{:?}", k),
                    };
                    if let Some(any) = AnySurrealType::from_cbor(v) {
                        write!(f, "{}: {}", key_str, any)?;
                    } else {
                        write!(f, "{}: {:?}", key_str, v)?;
                    }
                }
                write!(f, "}}")
            }
            Value::Tag(6, _) => write!(f, "NONE"),
            Value::Tag(8, inner) => {
                // Record ID: Tag(8, Array([Text(table), Text(id)]))
                if let Value::Array(parts) = inner.as_ref()
                    && let (Some(Value::Text(table)), Some(Value::Text(id))) =
                        (parts.first(), parts.get(1))
                {
                    return write!(f, "{}:{}", table, id);
                }
                write!(f, "{:?}", inner)
            }
            Value::Tag(10, inner) => {
                // Decimal
                if let Value::Text(s) = inner.as_ref() {
                    write!(f, "{}", s)
                } else {
                    write!(f, "{:?}", inner)
                }
            }
            Value::Tag(_, inner) => {
                if let Some(any) = AnySurrealType::from_cbor(inner) {
                    write!(f, "{}", any)
                } else {
                    write!(f, "{:?}", inner)
                }
            }
            other => write!(f, "{:?}", other),
        }
    }
}

impl TerminalRender for AnySurrealType {
    fn render(&self) -> String {
        use ciborium::Value;
        match &self.value {
            Value::Null | Value::Tag(6, _) => "-".to_string(),
            Value::Text(s) => s.clone(),
            Value::Bool(b) => b.to_string(),
            _ => format!("{}", self),
        }
    }

    fn color_hint(&self) -> Option<&'static str> {
        use ciborium::Value;
        match &self.value {
            Value::Bool(true) => Some("green"),
            Value::Bool(false) => Some("red"),
            Value::Null | Value::Tag(6, _) => Some("dim"),
            _ => None,
        }
    }
}

// TryFrom<AnySurrealType> impls for common types — enables AssociatedExpression::get()
macro_rules! impl_try_from_surreal {
    ($($ty:ty),*) => {
        $(
            impl TryFrom<AnySurrealType> for $ty {
                type Error = VantageError;
                fn try_from(val: AnySurrealType) -> Result<Self, Self::Error> {
                    val.try_get::<$ty>().ok_or_else(|| {
                        vantage_core::error!(
                            "Cannot convert AnySurrealType to target type",
                            target = std::any::type_name::<$ty>(),
                            value = format!("{}", val)
                        )
                    })
                }
            }
        )*
    };
}

impl_try_from_surreal!(i64, f64, bool, String, usize);

impl TryFrom<AnySurrealType> for Vec<AnySurrealType> {
    type Error = VantageError;
    fn try_from(val: AnySurrealType) -> Result<Self, Self::Error> {
        val.try_get::<Vec<AnySurrealType>>().ok_or_else(|| {
            vantage_core::error!(
                "Cannot convert AnySurrealType to Vec",
                value = format!("{}", val)
            )
        })
    }
}

impl TryFrom<AnySurrealType> for Record<AnySurrealType> {
    type Error = VantageError;
    fn try_from(val: AnySurrealType) -> Result<Self, Self::Error> {
        let value = val.into_value();
        let map = match value {
            ciborium::Value::Map(m) => m,
            ciborium::Value::Array(arr) => arr
                .into_iter()
                .find_map(|v| match v {
                    ciborium::Value::Map(m) => Some(m),
                    _ => None,
                })
                .ok_or_else(|| vantage_core::error!("Expected map in array result"))?,
            _ => return Err(vantage_core::error!("Expected map or array result")),
        };
        Ok(map
            .into_iter()
            .filter_map(|(k, v)| {
                let key = match k {
                    ciborium::Value::Text(s) => s,
                    _ => return None,
                };
                let val = AnySurrealType::from_cbor(&v)?;
                Some((key, val))
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_any_surreal_type_display() {
        let bool_val = AnySurrealType::from_cbor(&ciborium::Value::Bool(true)).unwrap();
        assert_eq!(format!("{}", bool_val), "true");
    }
}
