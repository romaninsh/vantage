//! `bson::DateTime` → MongoDB DateTime variant.

use super::{MongoType, MongoTypeDateTimeMarker};
use bson::Bson;

impl MongoType for bson::DateTime {
    type Target = MongoTypeDateTimeMarker;

    fn to_bson(&self) -> Bson {
        Bson::DateTime(*self)
    }

    /// Accepts the RFC 3339 text the CBOR bridge renders a DateTime as.
    fn from_bson(value: Bson) -> Option<Self> {
        match value {
            Bson::DateTime(dt) => Some(dt),
            Bson::String(s) => bson::DateTime::parse_rfc3339_str(s).ok(),
            _ => None,
        }
    }
}
