//! `bson::Decimal128` → MongoDB Decimal128 variant.

use super::{MongoType, MongoTypeDecimal128Marker};
use bson::{Bson, Decimal128};

impl MongoType for Decimal128 {
    type Target = MongoTypeDecimal128Marker;

    fn to_bson(&self) -> Bson {
        Bson::Decimal128(*self)
    }

    /// Accepts the numeric text the CBOR bridge renders a Decimal128 as.
    fn from_bson(value: Bson) -> Option<Self> {
        match value {
            Bson::Decimal128(d) => Some(d),
            Bson::String(s) => s.trim().parse().ok(),
            _ => None,
        }
    }
}
