//! `MongoId`: a document id that is an `ObjectId` or a string, the two kinds
//! `_id` almost always is.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use bson::{Bson, oid::ObjectId};
use serde::{Serialize, Serializer};

/// A MongoDB document identifier: an `ObjectId` or a `String`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum MongoId {
    ObjectId(ObjectId),
    String(String),
}

impl Serialize for MongoId {
    /// A plain string: the hex form of an `ObjectId`, or the raw string. JSON
    /// APIs want that, not the tagged `{ "$oid": "..." }` form.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl MongoId {
    /// An `ObjectId` or string value as an id; `None` for any other type.
    pub fn from_bson(value: &Bson) -> Option<Self> {
        match value {
            Bson::ObjectId(oid) => Some(MongoId::ObjectId(*oid)),
            Bson::String(s) => Some(MongoId::String(s.clone())),
            _ => None,
        }
    }

    pub fn to_bson(&self) -> Bson {
        match self {
            MongoId::ObjectId(oid) => Bson::ObjectId(*oid),
            MongoId::String(s) => Bson::String(s.clone()),
        }
    }
}

/// The other spelling of an id-shaped value: an `ObjectId` yields its hex
/// `String`, a `String` that parses as an `ObjectId` yields that `ObjectId`,
/// anything else yields `None`. Matching both spellings lets a hex id from a
/// text-typed surface find documents whose key is stored as an `ObjectId`.
pub(crate) fn id_variants(value: &Bson) -> Option<Bson> {
    match value {
        Bson::ObjectId(oid) => Some(Bson::String(oid.to_hex())),
        Bson::String(s) if s.len() == 24 => ObjectId::parse_str(s).ok().map(Bson::ObjectId),
        _ => None,
    }
}

/// Equality on `path`: a plain match, or `$in` over both id spellings.
pub(crate) fn eq_filter(path: &str, value: &Bson) -> bson::Document {
    match id_variants(value) {
        Some(alt) => bson::doc! { path: { "$in": [value.clone(), alt] } },
        None => bson::doc! { path: value.clone() },
    }
}

impl Hash for MongoId {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Discriminant ensures ObjectId("abc...") and String("abc...") don't collide
        std::mem::discriminant(self).hash(state);
        match self {
            MongoId::ObjectId(oid) => oid.hash(state),
            MongoId::String(s) => s.hash(state),
        }
    }
}

impl fmt::Display for MongoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MongoId::ObjectId(oid) => write!(f, "{}", oid),
            MongoId::String(s) => write!(f, "{}", s),
        }
    }
}

impl FromStr for MongoId {
    type Err = std::convert::Infallible;

    /// A 24-character hex string is an `ObjectId`, anything else a `String`.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(oid) = ObjectId::parse_str(s) {
            Ok(MongoId::ObjectId(oid))
        } else {
            Ok(MongoId::String(s.to_string()))
        }
    }
}

impl From<MongoId> for Bson {
    fn from(id: MongoId) -> Self {
        id.to_bson()
    }
}

impl From<ObjectId> for MongoId {
    fn from(oid: ObjectId) -> Self {
        MongoId::ObjectId(oid)
    }
}

impl From<String> for MongoId {
    /// A 24-character hex string is an `ObjectId`, anything else a `String`.
    fn from(s: String) -> Self {
        if s.len() == 24
            && let Ok(oid) = ObjectId::parse_str(&s)
        {
            return MongoId::ObjectId(oid);
        }
        MongoId::String(s)
    }
}

impl From<&str> for MongoId {
    /// A 24-character hex string is an `ObjectId`, anything else a `String`.
    fn from(s: &str) -> Self {
        if s.len() == 24
            && let Ok(oid) = ObjectId::parse_str(s)
        {
            return MongoId::ObjectId(oid);
        }
        MongoId::String(s.to_string())
    }
}
