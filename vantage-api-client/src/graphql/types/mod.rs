//! GraphQL Type System for Vantage.
//!
//! Uses `serde_json::Value` as the native wire type since GraphQL
//! responses and variables are JSON. The variant enum covers the five
//! GraphQL spec scalars (`Int`, `Float`, `String`, `Boolean`, `ID`) plus
//! the common custom scalars (`DateTime`, `Date`, `Time`, `Uuid`,
//! `Decimal`, `BigInt`, `Json`) and composites (`Object`, `Array`).
//!
//! Downstream crates can plug their own Rust types onto these variants
//! by implementing `GraphqlType`. To extend the *variant set* (e.g. for
//! geo types `Point`/`Polygon`), call `vantage_type_system!` again in a
//! sibling crate — `GraphqlSelect` and `GraphqlCondition` are generic
//! over the value type and come along for free.

use vantage_types::vantage_type_system;

vantage_type_system! {
    type_trait: GraphqlType,
    method_name: json,
    value_type: serde_json::Value,
    type_variants: [
        Null,
        Bool,      // Boolean
        Int,       // 32-bit signed (spec)
        BigInt,    // 64-bit signed (custom scalar)
        Float,     // 64-bit double
        String,    // text scalar
        Id,        // ID scalar (serialises as String over the wire)
        DateTime,  // RFC 3339
        Date,      // YYYY-MM-DD
        Time,      // HH:MM:SS
        Uuid,      // RFC 4122
        Decimal,   // arbitrary-precision (downstream-implemented)
        Json,      // opaque JSON (Hasura `jsonb`, etc.)
        Object,
        Array,
    ]
}

impl GraphqlTypeVariants {
    /// Best-effort detection from a raw JSON value. Strings could be any
    /// of `String`/`Id`/`DateTime`/`Uuid`/etc. — we return `String` and
    /// let the caller's `try_get::<T>()` decide (untyped values bypass
    /// the variant check, so chrono/uuid parsers still get their shot).
    pub fn from_json(value: &serde_json::Value) -> Option<Self> {
        match value {
            serde_json::Value::Null => Some(Self::Null),
            serde_json::Value::Bool(_) => Some(Self::Bool),
            serde_json::Value::Number(n) => {
                if n.is_f64() {
                    Some(Self::Float)
                } else {
                    Some(Self::BigInt)
                }
            }
            serde_json::Value::String(_) => Some(Self::String),
            serde_json::Value::Array(_) => Some(Self::Array),
            serde_json::Value::Object(_) => Some(Self::Object),
        }
    }
}

mod bool;
mod chrono;
mod numbers;
mod string;
mod uuid;
mod value;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bool_round_trip() {
        let v = AnyGraphqlType::new(true);
        assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::Bool));
        assert_eq!(v.try_get::<bool>(), Some(true));
    }

    #[test]
    fn i32_round_trip() {
        let v = AnyGraphqlType::new(42i32);
        assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::Int));
        assert_eq!(v.try_get::<i32>(), Some(42));
    }

    #[test]
    fn i64_round_trip() {
        let v = AnyGraphqlType::new(9_000_000_000i64);
        assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::BigInt));
        assert_eq!(v.try_get::<i64>(), Some(9_000_000_000));
    }

    #[test]
    fn f64_round_trip() {
        let v = AnyGraphqlType::new(3.14f64);
        assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::Float));
        assert_eq!(v.try_get::<f64>(), Some(3.14));
    }

    #[test]
    fn string_round_trip() {
        let v = AnyGraphqlType::new("hello".to_string());
        assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::String));
        assert_eq!(v.try_get::<String>(), Some("hello".into()));
    }

    #[test]
    fn type_mismatch_rejected() {
        let v = AnyGraphqlType::new("not a number".to_string());
        assert_eq!(v.try_get::<i64>(), None);
    }

    #[test]
    fn untyped_value_bypasses_variant_check() {
        let v = AnyGraphqlType::untyped(serde_json::json!(42));
        assert_eq!(v.try_get::<i64>(), Some(42));
    }

    #[test]
    fn datetime_round_trip() {
        use ::chrono::{DateTime, Utc};
        let now: DateTime<Utc> = "2026-05-16T12:00:00Z".parse().unwrap();
        let v = AnyGraphqlType::new(now);
        assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::DateTime));
        assert_eq!(v.try_get::<DateTime<Utc>>(), Some(now));
    }

    #[test]
    fn uuid_round_trip() {
        use ::uuid::Uuid;
        let id: Uuid = "550e8400-e29b-41d4-a716-446655440000".parse().unwrap();
        let v = AnyGraphqlType::new(id);
        assert_eq!(v.type_variant(), Some(GraphqlTypeVariants::Uuid));
        assert_eq!(v.try_get::<Uuid>(), Some(id));
    }

    #[test]
    fn option_some_round_trip() {
        let v = AnyGraphqlType::new(Some(42i64));
        assert_eq!(v.try_get::<Option<i64>>(), Some(Some(42)));
    }

    #[test]
    fn option_none_renders_null() {
        let v = AnyGraphqlType::new(None::<i64>);
        assert_eq!(*v.value(), serde_json::Value::Null);
        assert_eq!(v.try_get::<Option<i64>>(), Some(None));
    }
}
