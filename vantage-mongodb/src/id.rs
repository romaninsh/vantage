//! MongoDB ID type — wraps either `ObjectId` or `String`.
//!
//! MongoDB `_id` can be any BSON type, but in practice it's almost always
//! either an auto-generated `ObjectId` or a user-supplied string. `MongoId`
//! handles both transparently.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use bson::{Bson, oid::ObjectId};
use serde::{Serialize, Serializer};

/// A MongoDB document identifier — either an `ObjectId` or a `String`.
#[derive(Debug, Clone, Eq, PartialEq)]
pub enum MongoId {
    ObjectId(ObjectId),
    String(String),
}

impl Serialize for MongoId {
    /// Serializes as a plain string — the hex form for `ObjectId`, or the raw
    /// string value for `MongoId::String`. This matches how HTTP APIs typically
    /// want ids to appear in JSON: a single string rather than the tagged
    /// `{ "$oid": "..." }` form.
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl MongoId {
    /// Create from a BSON value. Returns `None` for unsupported types.
    pub fn from_bson(value: &Bson) -> Option<Self> {
        match value {
            Bson::ObjectId(oid) => Some(MongoId::ObjectId(*oid)),
            Bson::String(s) => Some(MongoId::String(s.clone())),
            _ => None,
        }
    }

    /// Convert to a BSON value for use in queries.
    pub fn to_bson(&self) -> Bson {
        match self {
            MongoId::ObjectId(oid) => Bson::ObjectId(*oid),
            MongoId::String(s) => Bson::String(s.clone()),
        }
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

    /// Parses a string as MongoId. If it's a valid 24-char hex string,
    /// treats it as ObjectId; otherwise stores as String.
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
    /// 24-character hex strings parse as `ObjectId`, anything else stays a plain
    /// `String`. Mirrors [`MongoId::from_str`] so `.get(id_string)` and similar
    /// call sites work without an explicit parse.
    fn from(s: String) -> Self {
        if s.len() == 24 {
            if let Ok(oid) = ObjectId::parse_str(&s) {
                return MongoId::ObjectId(oid);
            }
        }
        MongoId::String(s)
    }
}

impl From<&str> for MongoId {
    /// 24-character hex strings parse as `ObjectId`, anything else stays a plain
    /// `String`. Mirrors [`MongoId::from_str`].
    fn from(s: &str) -> Self {
        if s.len() == 24 {
            if let Ok(oid) = ObjectId::parse_str(s) {
                return MongoId::ObjectId(oid);
            }
        }
        MongoId::String(s.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn test_from_bson_objectid() {
        let oid = ObjectId::new();
        let id = MongoId::from_bson(&Bson::ObjectId(oid)).unwrap();
        assert_eq!(id, MongoId::ObjectId(oid));
    }

    #[test]
    fn test_from_bson_string() {
        let id = MongoId::from_bson(&Bson::String("flux_cupcake".into())).unwrap();
        assert_eq!(id, MongoId::String("flux_cupcake".into()));
    }

    #[test]
    fn test_from_bson_unsupported() {
        assert!(MongoId::from_bson(&Bson::Int32(42)).is_none());
    }

    #[test]
    fn test_to_bson() {
        let oid = ObjectId::new();
        assert_eq!(MongoId::ObjectId(oid).to_bson(), Bson::ObjectId(oid));
        assert_eq!(
            MongoId::String("abc".into()).to_bson(),
            Bson::String("abc".into())
        );
    }

    #[test]
    fn test_display() {
        let oid = ObjectId::new();
        assert_eq!(MongoId::ObjectId(oid).to_string(), oid.to_string());
        assert_eq!(MongoId::String("hello".into()).to_string(), "hello");
    }

    #[test]
    fn test_from_str_objectid() {
        let oid = ObjectId::new();
        let parsed: MongoId = oid.to_hex().parse().unwrap();
        assert_eq!(parsed, MongoId::ObjectId(oid));
    }

    #[test]
    fn test_from_str_string() {
        let parsed: MongoId = "flux_cupcake".parse().unwrap();
        assert_eq!(parsed, MongoId::String("flux_cupcake".into()));
    }

    #[test]
    fn test_hash_distinct() {
        let mut set = HashSet::new();
        set.insert(MongoId::String("abc".into()));
        set.insert(MongoId::String("def".into()));
        set.insert(MongoId::ObjectId(ObjectId::new()));
        assert_eq!(set.len(), 3);
    }

    #[test]
    fn test_from_conversions() {
        let id: MongoId = ObjectId::new().into();
        assert!(matches!(id, MongoId::ObjectId(_)));

        let id: MongoId = "hello".into();
        assert!(matches!(id, MongoId::String(_)));

        let id: MongoId = String::from("world").into();
        assert!(matches!(id, MongoId::String(_)));
    }
}
