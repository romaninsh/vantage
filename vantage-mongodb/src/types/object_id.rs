//! ObjectId → MongoDB ObjectId variant.

use super::{MongoType, MongoTypeObjectIdMarker};
use bson::Bson;

impl MongoType for bson::oid::ObjectId {
    type Target = MongoTypeObjectIdMarker;

    fn to_bson(&self) -> Bson {
        Bson::ObjectId(*self)
    }

    fn from_bson(value: Bson) -> Option<Self> {
        match value {
            Bson::ObjectId(oid) => Some(oid),
            Bson::String(s) => s.parse().ok(),
            _ => None,
        }
    }
}
