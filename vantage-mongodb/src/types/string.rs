//! String types → MongoDB String variant

use super::{MongoType, MongoTypeStringMarker};
use bson::Bson;

impl MongoType for String {
    type Target = MongoTypeStringMarker;

    fn to_bson(&self) -> Bson {
        Bson::String(self.clone())
    }

    fn from_bson(value: Bson) -> Option<Self> {
        match value {
            Bson::String(s) => Some(s),
            _ => None,
        }
    }
}

impl MongoType for &'static str {
    type Target = MongoTypeStringMarker;

    fn to_bson(&self) -> Bson {
        Bson::String(self.to_string())
    }

    fn from_bson(_value: Bson) -> Option<Self> {
        None // cannot produce &'static str from arbitrary data
    }
}
