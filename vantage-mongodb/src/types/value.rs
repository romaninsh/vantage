//! AnyMongoType extras: untyped constructor, From impls, Expressive impls.

use super::{AnyMongoType, MongoType, MongoTypeArrayMarker, MongoTypeNullMarker};
use vantage_expressions::{Expression, Expressive, ExpressiveEnum};

impl AnyMongoType {
    /// Create an AnyMongoType with no type marker. Used for values coming
    /// back from the database where we don't know the original type.
    /// `try_get` on these values bypasses variant checking.
    pub fn untyped(value: bson::Bson) -> Self {
        Self {
            value,
            type_variant: None,
        }
    }
}

/// AnyMongoType is itself a MongoType — passthrough for type-erased values.
impl MongoType for AnyMongoType {
    type Target = MongoTypeNullMarker;

    fn to_bson(&self) -> bson::Bson {
        self.value().clone()
    }

    fn from_bson(value: bson::Bson) -> Option<Self> {
        AnyMongoType::from_bson(&value)
    }
}

/// Vec<AnyMongoType> is a MongoType — used by column_table_values_expr to
/// return a list of column values wrapped in an AnyMongoType.
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

// From impls for common types
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

impl_from_for_any_mongo!(i32, i64, f64, bool, String, bson::oid::ObjectId);

impl From<&str> for AnyMongoType {
    /// Converts a `&str` into an `AnyMongoType`. 24-character hex strings are
    /// treated as `ObjectId`, anything else stays a plain `String`. This mirrors
    /// [`MongoId::from_str`] and makes id comparisons like `column.eq("68c1...")`
    /// work without an explicit `MongoId::parse` step.
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

// Expressive impls — allows passing scalars directly into mongo expressions
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
// Into<serde_json::Value> for AnyTable::from_table() bridge.
// Uses serde: Bson -> serde_json::Value via Bson's Serialize impl (extended
// JSON for ObjectId/DateTime/…), and serde_json::Value -> Bson via Bson's
// Deserialize impl. Neither direction panics: the rare failure falls back
// to the structural bson<->cbor bridge instead of `.expect()`.
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
        // Deserialize from the borrow (`&Value` is a `Deserializer`) so the
        // value stays available for the fallback: Bson's Deserialize rejects
        // malformed extended JSON (e.g. `{"$oid": "not-a-hex-id"}`), and we
        // fall back to the structural conversion instead of panicking.
        use serde::Deserialize as _;
        let bson = bson::Bson::deserialize(&val).unwrap_or_else(|_| {
            crate::types::cbor::cbor_to_bson(&vantage_types::json_to_cbor(val))
        });
        AnyMongoType::untyped(bson)
    }
}

// CBOR bridge for `AnyTable` interop — the same structural bson<->cbor
// bridge the Vista source uses, so both surfaces render values identically
// (ObjectId as its hex string, Binary as bytes). The previous serde_json
// round-trip collapsed any tagged CBOR value to Null.
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
