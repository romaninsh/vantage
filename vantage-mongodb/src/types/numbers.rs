//! Numeric type implementations for MongoDB.
//!
//! - i32 → MongoType::Int32
//! - i64 → MongoType::Int64
//! - f64 → MongoType::Double

use super::{MongoType, MongoTypeDoubleMarker, MongoTypeInt32Marker, MongoTypeInt64Marker};
use bson::Bson;

impl MongoType for i32 {
    type Target = MongoTypeInt32Marker;

    fn to_bson(&self) -> Bson {
        Bson::Int32(*self)
    }

    fn from_bson(value: Bson) -> Option<Self> {
        match value {
            Bson::Int32(i) => Some(i),
            Bson::Int64(i) => i32::try_from(i).ok(),
            _ => None,
        }
    }
}

impl MongoType for i64 {
    type Target = MongoTypeInt64Marker;

    fn to_bson(&self) -> Bson {
        Bson::Int64(*self)
    }

    fn from_bson(value: Bson) -> Option<Self> {
        match value {
            Bson::Int64(i) => Some(i),
            Bson::Int32(i) => Some(i as i64),
            _ => None,
        }
    }
}

impl MongoType for f64 {
    type Target = MongoTypeDoubleMarker;

    fn to_bson(&self) -> Bson {
        Bson::Double(*self)
    }

    fn from_bson(value: Bson) -> Option<Self> {
        match value {
            Bson::Double(d) => Some(d),
            Bson::Int32(i) => Some(i as f64),
            Bson::Int64(i) => Some(i as f64),
            _ => None,
        }
    }
}

impl<T: MongoType> MongoType for Option<T> {
    type Target = T::Target;

    fn to_bson(&self) -> Bson {
        match self {
            Some(v) => v.to_bson(),
            None => Bson::Null,
        }
    }

    fn from_bson(value: Bson) -> Option<Self> {
        match value {
            Bson::Null => Some(None),
            other => T::from_bson(other).map(Some),
        }
    }
}
