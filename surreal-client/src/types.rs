//! # SurrealDB Type System using vantage-types
//!
//! This module defines the SurrealDB type system using the vantage-types framework,
//! providing type-safe operations with automatic CBOR serialization.

use vantage_types::vantage_type_system;

// Generate the SurrealType system using vantage-types macro
vantage_type_system! {
    type_trait: SurrealType,
    method_name: cbor,
    value_type: ciborium::Value,
    type_variants: [Any, Int, Float, Decimal, String, Bool, DateTime, Duration, Json, Geo]
}

// The macro generates these types automatically - no need to re-export

// Override the macro-generated variant detection with SurrealDB-specific logic
impl SurrealTypeVariants {
    pub fn from_cbor(value: &ciborium::Value) -> Option<Self> {
        use ciborium::Value::*;
        match value {
            Null => Some(SurrealTypeVariants::Any),
            Bool(_) => Some(SurrealTypeVariants::Bool),
            Integer(_) => Some(SurrealTypeVariants::Int),
            Float(_) => Some(SurrealTypeVariants::Float),
            Text(_s) => Some(SurrealTypeVariants::String),
            Bytes(_) => Some(SurrealTypeVariants::String), // Convert bytes to hex string
            Array(_) => Some(SurrealTypeVariants::Json),
            Map(_) => Some(SurrealTypeVariants::Json),
            Tag(12, _) => Some(SurrealTypeVariants::DateTime), // SurrealDB DateTime tag
            Tag(14, _) => Some(SurrealTypeVariants::Duration), // SurrealDB Duration tag
            Tag(200, _) => Some(SurrealTypeVariants::Decimal), // Custom decimal tag
            Tag(300, _) => Some(SurrealTypeVariants::Geo),     // Custom geo tag
            Tag(_, boxed_value) => Self::from_cbor(boxed_value), // Recurse into tagged values
            _ => None,
        }
    }
}

// Include type implementations
mod standard;
// standard types are imported individually as needed

mod datetime;
pub use datetime::*;

mod value;

#[cfg(feature = "decimal")]
mod decimal;

mod geo;

// Export key types for public use
pub use standard::{Any, RId};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_any_surreal_type_creation() {
        let s = "hello".to_string();
        let any = AnySurrealType::new(s.clone());

        assert_eq!(any.type_variant(), Some(SurrealTypeVariants::String));
        assert!(any.try_get::<String>().is_some());
    }

    #[test]
    fn test_variant_detection() {
        use ciborium::Value::*;

        assert_eq!(
            SurrealTypeVariants::from_cbor(&Text("hello".to_string())),
            Some(SurrealTypeVariants::String)
        );
        assert_eq!(
            SurrealTypeVariants::from_cbor(&Text("user:123".to_string())),
            Some(SurrealTypeVariants::String)
        );
        assert_eq!(
            SurrealTypeVariants::from_cbor(&Integer(42.into())),
            Some(SurrealTypeVariants::Int)
        );
        assert_eq!(
            SurrealTypeVariants::from_cbor(&Bool(true)),
            Some(SurrealTypeVariants::Bool)
        );
        assert_eq!(
            SurrealTypeVariants::from_cbor(&Null),
            Some(SurrealTypeVariants::Any)
        );
    }
}
