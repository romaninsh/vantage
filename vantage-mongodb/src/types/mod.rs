//! MongoDB Type System for Vantage Framework
//!
//! Uses `bson::Bson` as the native value type with type variants matching
//! MongoDB's BSON type set.

use vantage_core::VantageError;
use vantage_types::{Record, vantage_type_system};

vantage_type_system! {
    type_trait: MongoType,
    method_name: bson,
    value_type: bson::Bson,
    type_variants: [
        Null,
        Bool,
        Int32,
        Int64,
        Double,
        String,
        ObjectId,
        DateTime,
        Binary,
        Array,
        Document,
        Decimal128,
        Regex,
        Timestamp
    ]
}

impl MongoTypeVariants {
    pub fn from_bson(value: &bson::Bson) -> Option<Self> {
        match value {
            bson::Bson::Null => Some(Self::Null),
            bson::Bson::Boolean(_) => Some(Self::Bool),
            bson::Bson::Int32(_) => Some(Self::Int32),
            bson::Bson::Int64(_) => Some(Self::Int64),
            bson::Bson::Double(_) => Some(Self::Double),
            bson::Bson::String(_) => Some(Self::String),
            bson::Bson::ObjectId(_) => Some(Self::ObjectId),
            bson::Bson::DateTime(_) => Some(Self::DateTime),
            bson::Bson::Binary(_) => Some(Self::Binary),
            bson::Bson::Array(_) => Some(Self::Array),
            bson::Bson::Document(_) => Some(Self::Document),
            bson::Bson::Decimal128(_) => Some(Self::Decimal128),
            bson::Bson::RegularExpression(_) => Some(Self::Regex),
            bson::Bson::Timestamp(_) => Some(Self::Timestamp),
            _ => None,
        }
    }
}

mod bool;
mod numbers;
mod object_id;
mod string;
mod value;

impl std::fmt::Display for AnyMongoType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.value {
            bson::Bson::Null => write!(f, "null"),
            bson::Bson::Boolean(b) => write!(f, "{}", b),
            bson::Bson::Int32(i) => write!(f, "{}", i),
            bson::Bson::Int64(i) => write!(f, "{}", i),
            bson::Bson::Double(d) => write!(f, "{}", d),
            bson::Bson::String(s) => write!(f, "{:?}", s),
            bson::Bson::ObjectId(oid) => write!(f, "ObjectId({:?})", oid.to_string()),
            bson::Bson::DateTime(dt) => write!(f, "{}", dt),
            bson::Bson::Array(arr) => write!(f, "{:?}", arr),
            bson::Bson::Document(doc) => write!(f, "{}", doc),
            other => write!(f, "{:?}", other),
        }
    }
}

// TryFrom<AnyMongoType> for common scalar types
macro_rules! impl_try_from_mongo {
    ($($ty:ty),*) => {
        $(
            impl TryFrom<AnyMongoType> for $ty {
                type Error = VantageError;
                fn try_from(val: AnyMongoType) -> Result<Self, Self::Error> {
                    val.try_get::<$ty>().ok_or_else(|| {
                        vantage_core::error!(
                            "Cannot convert AnyMongoType to target type",
                            target = std::any::type_name::<$ty>(),
                            value = format!("{}", val)
                        )
                    })
                }
            }
        )*
    };
}

impl_try_from_mongo!(i32, i64, f64, bool, String);

impl TryFrom<AnyMongoType> for bson::oid::ObjectId {
    type Error = VantageError;
    fn try_from(val: AnyMongoType) -> Result<Self, Self::Error> {
        val.try_get::<bson::oid::ObjectId>().ok_or_else(|| {
            vantage_core::error!(
                "Cannot convert AnyMongoType to ObjectId",
                value = format!("{}", val)
            )
        })
    }
}

impl TryFrom<AnyMongoType> for Record<AnyMongoType> {
    type Error = VantageError;
    fn try_from(val: AnyMongoType) -> Result<Self, Self::Error> {
        let value = val.into_value();
        match value {
            bson::Bson::Document(doc) => Ok(doc
                .into_iter()
                .map(|(k, v)| (k, AnyMongoType::untyped(v)))
                .collect()),
            bson::Bson::Array(arr) => {
                // Extract first document from array result
                let doc = arr
                    .into_iter()
                    .find_map(|v| match v {
                        bson::Bson::Document(d) => Some(d),
                        _ => None,
                    })
                    .ok_or_else(|| vantage_core::error!("Expected document in array result"))?;
                Ok(doc
                    .into_iter()
                    .map(|(k, v)| (k, AnyMongoType::untyped(v)))
                    .collect())
            }
            _ => Err(vantage_core::error!("Expected document or array result")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_i64_round_trip() {
        let val = AnyMongoType::new(42i64);
        assert_eq!(val.type_variant(), Some(MongoTypeVariants::Int64));
        assert_eq!(val.try_get::<i64>(), Some(42));
    }

    #[test]
    fn test_i32_round_trip() {
        let val = AnyMongoType::new(7i32);
        assert_eq!(val.type_variant(), Some(MongoTypeVariants::Int32));
        assert_eq!(val.try_get::<i32>(), Some(7));
    }

    #[test]
    fn test_string_round_trip() {
        let val = AnyMongoType::new("hello".to_string());
        assert_eq!(val.type_variant(), Some(MongoTypeVariants::String));
        assert_eq!(val.try_get::<String>(), Some("hello".to_string()));
    }

    #[test]
    fn test_bool_round_trip() {
        let val = AnyMongoType::new(true);
        assert_eq!(val.type_variant(), Some(MongoTypeVariants::Bool));
        assert_eq!(val.try_get::<bool>(), Some(true));
    }

    #[test]
    fn test_f64_round_trip() {
        let val = AnyMongoType::new(2.72f64);
        assert_eq!(val.type_variant(), Some(MongoTypeVariants::Double));
        assert_eq!(val.try_get::<f64>(), Some(2.72));
    }

    #[test]
    fn test_objectid_round_trip() {
        let oid = bson::oid::ObjectId::new();
        let val = AnyMongoType::new(oid);
        assert_eq!(val.type_variant(), Some(MongoTypeVariants::ObjectId));
        assert_eq!(val.try_get::<bson::oid::ObjectId>(), Some(oid));
    }

    #[test]
    fn test_type_mismatch_string_as_i64() {
        let val = AnyMongoType::new("hello".to_string());
        assert_eq!(val.try_get::<i64>(), None);
    }

    #[test]
    fn test_type_mismatch_i64_as_string() {
        let val = AnyMongoType::new(42i64);
        assert_eq!(val.try_get::<String>(), None);
    }

    #[test]
    fn test_untyped_permissive() {
        let val = AnyMongoType {
            value: bson::Bson::Int64(42),
            type_variant: None,
        };
        assert_eq!(val.try_get::<i64>(), Some(42));
        // Untyped: no variant check, so cross-family conversion depends on from_bson impl
        assert_eq!(val.try_get::<String>(), None); // Bson::Int64 can't become String
    }

    #[test]
    fn test_option_some() {
        let val = AnyMongoType::new(Some(42i64));
        assert_eq!(val.try_get::<Option<i64>>(), Some(Some(42)));
    }

    #[test]
    fn test_option_none() {
        let val = AnyMongoType::new(None::<i64>);
        assert_eq!(*val.value(), bson::Bson::Null);
        assert_eq!(val.try_get::<Option<i64>>(), Some(None));
    }
}
