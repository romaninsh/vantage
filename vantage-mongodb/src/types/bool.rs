//! Boolean → MongoDB Bool variant.

use super::{MongoType, MongoTypeBoolMarker};
use bson::Bson;

impl MongoType for bool {
    type Target = MongoTypeBoolMarker;

    fn to_bson(&self) -> Bson {
        Bson::Boolean(*self)
    }

    fn from_bson(value: Bson) -> Option<Self> {
        match value {
            Bson::Boolean(b) => Some(b),
            _ => None,
        }
    }
}
