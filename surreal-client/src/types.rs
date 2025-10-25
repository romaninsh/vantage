//! # SurrealDB Type System
//!
//! This module defines the type system for SurrealDB values, providing a trait-based
//! approach to type-safe column operations.

use serde_json::Value;

/// Enum representing SurrealDB column types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurrealTypeEnum {
    Any,
    Int,
    Float,
    #[cfg(feature = "decimal")]
    Decimal,
    String,
    Bool,
    DateTime,
    Duration,
}

impl SurrealTypeEnum {
    /// Get the SurrealDB type name as a string
    pub fn type_name(&self) -> &'static str {
        match self {
            SurrealTypeEnum::Any => "any",
            SurrealTypeEnum::Int => "int",
            SurrealTypeEnum::Float => "float",
            #[cfg(feature = "decimal")]
            SurrealTypeEnum::Decimal => "decimal",
            SurrealTypeEnum::String => "string",
            SurrealTypeEnum::Bool => "bool",
            SurrealTypeEnum::DateTime => "datetime",
            SurrealTypeEnum::Duration => "duration",
        }
    }
}

/// Marker trait for SurrealDB types
/// Describes how a Rust type maps to SurrealDB type system
pub trait SurrealType: Send + Sync + std::fmt::Debug + 'static {
    /// The SurrealDB type enum for this type
    fn type_enum() -> SurrealTypeEnum;

    /// The SurrealDB type name (e.g., "string", "int", "datetime")
    fn type_name() -> &'static str {
        Self::type_enum().type_name()
    }

    /// Convert to serde_json::Value for transport
    fn to_value(&self) -> Value
    where
        Self: Sized;

    /// Parse from serde_json::Value (optional, for deserialization)
    fn from_value(_value: Value) -> Result<Self, String>
    where
        Self: Sized,
    {
        Err(format!(
            "from_value not implemented for {}",
            Self::type_name()
        ))
    }
}

// Base type implementations

impl SurrealType for String {
    fn type_enum() -> SurrealTypeEnum {
        SurrealTypeEnum::String
    }

    fn to_value(&self) -> Value {
        Value::String(self.clone())
    }

    fn from_value(v: Value) -> Result<Self, String> {
        v.as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| "Not a string".to_string())
    }
}

impl SurrealType for i64 {
    fn type_enum() -> SurrealTypeEnum {
        SurrealTypeEnum::Int
    }

    fn to_value(&self) -> Value {
        Value::Number((*self).into())
    }

    fn from_value(v: Value) -> Result<Self, String> {
        v.as_i64().ok_or_else(|| "Not an int".to_string())
    }
}

impl SurrealType for f64 {
    fn type_enum() -> SurrealTypeEnum {
        SurrealTypeEnum::Float
    }

    fn to_value(&self) -> Value {
        Value::Number(serde_json::Number::from_f64(*self).unwrap_or_else(|| 0.into()))
    }

    fn from_value(v: Value) -> Result<Self, String> {
        v.as_f64().ok_or_else(|| "Not a float".to_string())
    }
}

impl SurrealType for bool {
    fn type_enum() -> SurrealTypeEnum {
        SurrealTypeEnum::Bool
    }

    fn to_value(&self) -> Value {
        Value::Bool(*self)
    }

    fn from_value(v: Value) -> Result<Self, String> {
        v.as_bool().ok_or_else(|| "Not a bool".to_string())
    }
}

// Marker type for Any (untyped columns)

/// Represents an untyped SurrealDB value
#[derive(Debug, Clone)]
pub struct Any;

impl SurrealType for Any {
    fn type_enum() -> SurrealTypeEnum {
        SurrealTypeEnum::Any
    }

    fn to_value(&self) -> Value {
        Value::Null
    }

    fn from_value(_v: Value) -> Result<Self, String> {
        Ok(Any)
    }
}

// Wrapper types for SurrealDB-specific types

/// Represents a SurrealDB decimal value
/// Uses rust_decimal for precise decimal arithmetic
#[cfg(feature = "decimal")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decimal(pub rust_decimal::Decimal);

#[cfg(feature = "decimal")]
impl SurrealType for Decimal {
    fn type_enum() -> SurrealTypeEnum {
        SurrealTypeEnum::Decimal
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.to_string())
    }

    fn from_value(v: Value) -> Result<Self, String> {
        v.as_str()
            .and_then(|s| s.parse::<rust_decimal::Decimal>().ok())
            .map(Decimal)
            .ok_or_else(|| "Not a valid decimal".to_string())
    }
}

/// Represents a SurrealDB datetime value
/// Uses chrono::DateTime<Utc> for proper timezone handling
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateTime(pub chrono::DateTime<chrono::Utc>);

impl DateTime {
    pub fn new(dt: chrono::DateTime<chrono::Utc>) -> Self {
        Self(dt)
    }

    pub fn now() -> Self {
        Self(chrono::Utc::now())
    }
}

impl SurrealType for DateTime {
    fn type_enum() -> SurrealTypeEnum {
        SurrealTypeEnum::DateTime
    }

    fn to_value(&self) -> Value {
        Value::String(self.0.to_rfc3339())
    }

    fn from_value(v: Value) -> Result<Self, String> {
        v.as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| DateTime(dt.with_timezone(&chrono::Utc)))
            .ok_or_else(|| "Not a valid datetime string".to_string())
    }
}

/// Represents a SurrealDB duration value
/// Uses std::time::Duration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duration(pub std::time::Duration);

impl Duration {
    pub fn new(duration: std::time::Duration) -> Self {
        Self(duration)
    }

    pub fn from_secs(secs: u64) -> Self {
        Self(std::time::Duration::from_secs(secs))
    }

    pub fn from_millis(millis: u64) -> Self {
        Self(std::time::Duration::from_millis(millis))
    }
}

impl SurrealType for Duration {
    fn type_enum() -> SurrealTypeEnum {
        SurrealTypeEnum::Duration
    }

    fn to_value(&self) -> Value {
        // Format as "Xs" for seconds, "Xms" for milliseconds, etc.
        let secs = self.0.as_secs();
        let millis = self.0.subsec_millis();

        let duration_str = if secs > 0 && millis > 0 {
            format!("{}s{}ms", secs, millis)
        } else if secs > 0 {
            format!("{}s", secs)
        } else {
            format!("{}ms", millis)
        };

        Value::String(duration_str)
    }

    fn from_value(v: Value) -> Result<Self, String> {
        v.as_str()
            .and_then(|s| {
                // Simple parser for "Xs", "Xms", "Xs Xms" formats
                let mut secs = 0u64;
                let mut millis = 0u64;

                for part in s.split_whitespace() {
                    if let Some(num_str) = part.strip_suffix("s") {
                        if let Some(ms_str) = num_str.strip_suffix("m") {
                            millis = ms_str.parse().ok()?;
                        } else {
                            secs = num_str.parse().ok()?;
                        }
                    } else if let Some(num_str) = part.strip_suffix("ms") {
                        millis = num_str.parse().ok()?;
                    }
                }

                Some(Duration(
                    std::time::Duration::from_secs(secs) + std::time::Duration::from_millis(millis),
                ))
            })
            .ok_or_else(|| "Not a valid duration string".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, TimeZone};

    #[test]
    fn test_type_enum_names() {
        assert_eq!(SurrealTypeEnum::String.type_name(), "string");
        assert_eq!(SurrealTypeEnum::Int.type_name(), "int");
        assert_eq!(SurrealTypeEnum::Float.type_name(), "float");
        #[cfg(feature = "decimal")]
        assert_eq!(SurrealTypeEnum::Decimal.type_name(), "decimal");
        assert_eq!(SurrealTypeEnum::Bool.type_name(), "bool");
        assert_eq!(SurrealTypeEnum::DateTime.type_name(), "datetime");
        assert_eq!(SurrealTypeEnum::Duration.type_name(), "duration");
    }

    #[test]
    fn test_string_type() {
        let s = "hello".to_string();
        assert_eq!(String::type_enum(), SurrealTypeEnum::String);
        assert_eq!(s.to_value(), Value::String("hello".to_string()));

        let parsed = String::from_value(Value::String("world".to_string())).unwrap();
        assert_eq!(parsed, "world");
    }

    #[test]
    fn test_int_type() {
        let n = 42i64;
        assert_eq!(i64::type_enum(), SurrealTypeEnum::Int);
        assert_eq!(n.to_value(), Value::Number(42.into()));

        let parsed = i64::from_value(Value::Number(100.into())).unwrap();
        assert_eq!(parsed, 100);
    }

    #[test]
    fn test_float_type() {
        let _f = 3.14f64;
        assert_eq!(f64::type_enum(), SurrealTypeEnum::Float);

        let parsed =
            f64::from_value(Value::Number(serde_json::Number::from_f64(2.5).unwrap())).unwrap();
        assert_eq!(parsed, 2.5);
    }

    #[test]
    fn test_bool_type() {
        let b = true;
        assert_eq!(bool::type_enum(), SurrealTypeEnum::Bool);
        assert_eq!(b.to_value(), Value::Bool(true));

        let parsed = bool::from_value(Value::Bool(false)).unwrap();
        assert!(!parsed);
    }

    #[test]
    fn test_datetime_type() {
        let dt = DateTime::new(chrono::Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap());
        assert_eq!(DateTime::type_enum(), SurrealTypeEnum::DateTime);

        let value = dt.to_value();
        assert!(value.is_string());

        let parsed = DateTime::from_value(value).unwrap();
        assert_eq!(parsed.0.year(), 2024);
    }

    #[test]
    fn test_duration_type() {
        let dur = Duration::from_secs(60);
        assert_eq!(Duration::type_enum(), SurrealTypeEnum::Duration);

        let value = dur.to_value();
        assert_eq!(value, Value::String("60s".to_string()));

        let parsed = Duration::from_value(Value::String("30s".to_string())).unwrap();
        assert_eq!(parsed.0.as_secs(), 30);
    }

    #[test]
    fn test_duration_with_millis() {
        let dur = Duration::new(std::time::Duration::from_millis(1500));
        let value = dur.to_value();
        assert_eq!(value, Value::String("1s500ms".to_string()));
    }
}
