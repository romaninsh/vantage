//! SurrealDB Type System for Vantage Framework
//!
//! This module provides a SurrealDB-specific type system using the vantage-types framework.
//! It defines the core SurrealType trait and AnySurrealType for type-erased operations.

use vantage_types::vantage_type_system;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_any_surreal_type_display() {
        let bool_val = AnySurrealType::from_cbor(&ciborium::Value::Bool(true)).unwrap();
        assert_eq!(format!("{}", bool_val), "true");
    }
}
