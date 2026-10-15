//! The MongoDB type system: `bson::Bson` is the native value, with one type
//! variant per BSON type the framework distinguishes.

use vantage_core::VantageError;
use vantage_types::{Record, vantage_type_system};

vantage_type_system! {
    type_trait: MongoType,
    method_name: bson,
    value_type: bson::Bson,
    null_when: bson::Bson::Null,
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
pub mod cbor;
mod datetime;
mod decimal;
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

/// A document becomes a record; from an array, the first document does.
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
