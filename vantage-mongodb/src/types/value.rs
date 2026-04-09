//! AnyMongoType extras: untyped constructor, From impls, Expressive impls.

use super::{AnyMongoType, MongoType, MongoTypeNullMarker};
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
    fn from(val: &str) -> Self {
        AnyMongoType::new(val.to_string())
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
// Uses serde round-trip: Bson -> serde_json::Value via Bson's Serialize impl,
// and serde_json::Value -> Bson via Bson's Deserialize impl.
impl From<AnyMongoType> for serde_json::Value {
    fn from(val: AnyMongoType) -> Self {
        // Bson implements Serialize, serde_json::Value implements Deserialize
        serde_json::to_value(val.into_value()).unwrap_or(serde_json::Value::Null)
    }
}

impl From<serde_json::Value> for AnyMongoType {
    fn from(val: serde_json::Value) -> Self {
        // serde_json::Value implements Serialize, Bson implements Deserialize
        let bson: bson::Bson = serde_json::from_value(val).unwrap_or(bson::Bson::Null);
        AnyMongoType::untyped(bson)
    }
}
