//! AnyMongoType extras: untyped constructor, From impls, Expressive impls.

use super::{AnyMongoType, MongoType, MongoTypeArrayMarker, MongoTypeNullMarker};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

impl AnyMongoType {
    /// A value with no type marker, as read back from the database.
    /// `try_get` on it skips the variant check.
    pub fn untyped(value: bson::Bson) -> Self {
        Self {
            value,
            type_variant: None,
        }
    }
}

impl MongoType for AnyMongoType {
    type Target = MongoTypeNullMarker;

    fn to_bson(&self) -> bson::Bson {
        self.value().clone()
    }

    fn from_bson(value: bson::Bson) -> Option<Self> {
        AnyMongoType::from_bson(&value)
    }
}

// A column's values travel as one array value (`column_table_values_expr`).
impl MongoType for Vec<AnyMongoType> {
    type Target = MongoTypeArrayMarker;

    fn to_bson(&self) -> bson::Bson {
        bson::Bson::Array(self.iter().map(|v| v.value().clone()).collect())
    }

    fn from_bson(value: bson::Bson) -> Option<Self> {
        match value {
            bson::Bson::Array(arr) => Some(arr.into_iter().map(AnyMongoType::untyped).collect()),
            _ => None,
        }
    }
}

impl TryFrom<AnyMongoType> for Vec<AnyMongoType> {
    type Error = vantage_core::VantageError;
    fn try_from(val: AnyMongoType) -> Result<Self, Self::Error> {
        val.try_get::<Vec<AnyMongoType>>().ok_or_else(|| {
            vantage_core::error!(
                "Cannot convert AnyMongoType to Vec<AnyMongoType>",
                value = format!("{}", val)
            )
        })
    }
}

macro_rules! impl_from_for_any_mongo {
    ($($ty:ty),*) => {
        $(
            impl From<$ty> for AnyMongoType {
                fn from(val: $ty) -> Self {
                    AnyMongoType::new(val)
                }
            }
        )*
    };
}

impl_from_for_any_mongo!(
    i32,
    i64,
    f64,
    bool,
    String,
    bson::oid::ObjectId,
    bson::DateTime,
    bson::Decimal128
);

impl From<&str> for AnyMongoType {
    /// A 24-character hex string is an `ObjectId`, so `column.eq("68c1...")`
    /// compares ids without an explicit parse; anything else is a `String`.
    fn from(val: &str) -> Self {
        if val.len() == 24
            && let Ok(oid) = bson::oid::ObjectId::parse_str(val)
        {
            return AnyMongoType::new(oid);
        }
        AnyMongoType::new(val.to_string())
    }
}

impl From<crate::id::MongoId> for AnyMongoType {
    fn from(val: crate::id::MongoId) -> Self {
        match val {
            crate::id::MongoId::ObjectId(oid) => AnyMongoType::new(oid),
            crate::id::MongoId::String(s) => AnyMongoType::new(s),
        }
    }
}

macro_rules! impl_expressive_for_mongo_scalar {
    ($($ty:ty),*) => {
        $(
            impl Expressive<AnyMongoType> for $ty {
                fn expr(&self) -> Expression<AnyMongoType> {
                    Expression::new(
                        "{}",
                        vec![ExpressiveEnum::Scalar(AnyMongoType::new_ref(self))],
                    )
                }
            }
        )*
    };
}

impl_expressive_for_mongo_scalar!(i32, i64, f64, bool, String);

impl Expressive<AnyMongoType> for bson::oid::ObjectId {
    fn expr(&self) -> Expression<AnyMongoType> {
        Expression::new("{}", vec![ExpressiveEnum::Scalar(AnyMongoType::new(*self))])
    }
}

impl Expressive<AnyMongoType> for &str {
    fn expr(&self) -> Expression<AnyMongoType> {
        Expression::new(
            "{}",
            vec![ExpressiveEnum::Scalar(AnyMongoType::from(*self))],
        )
    }
}

impl Expressive<AnyMongoType> for AnyMongoType {
    fn expr(&self) -> Expression<AnyMongoType> {
        Expression::new("{}", vec![ExpressiveEnum::Scalar(self.clone())])
    }
}

// The `AnyTable::from_table()` bridge. BSON serializes to extended JSON
// (ObjectId, DateTime, ...) and back through serde; a value serde rejects
// falls back to the structural bson<->cbor bridge instead of panicking.
impl From<AnyMongoType> for serde_json::Value {
    fn from(val: AnyMongoType) -> Self {
        let bson = val.into_value();
        serde_json::to_value(&bson).unwrap_or_else(|_| {
            vantage_types::cbor_to_json(
                &vantage_types::PlainDialect,
                crate::types::cbor::bson_to_cbor(&bson),
            )
        })
    }
}

impl From<serde_json::Value> for AnyMongoType {
    fn from(val: serde_json::Value) -> Self {
        // Deserialize from the borrow so `val` is still there for the
        // fallback: malformed extended JSON (`{"$oid": "not-a-hex-id"}`) is
        // rejected.
        use serde::Deserialize as _;
        let bson = bson::Bson::deserialize(&val).unwrap_or_else(|_| {
            crate::types::cbor::cbor_to_bson(&vantage_types::json_to_cbor(val))
        });
        AnyMongoType::untyped(bson)
    }
}

// The same bson<->cbor bridge the vista source uses, so both surfaces render
// values identically (ObjectId as its hex string, Binary as bytes).
impl From<AnyMongoType> for ciborium::Value {
    fn from(val: AnyMongoType) -> Self {
        crate::types::cbor::bson_to_cbor(&val.into_value())
    }
}

impl From<ciborium::Value> for AnyMongoType {
    fn from(val: ciborium::Value) -> Self {
        AnyMongoType::untyped(crate::types::cbor::cbor_to_bson(&val))
    }
}
