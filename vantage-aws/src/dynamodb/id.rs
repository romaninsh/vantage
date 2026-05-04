//! DynamoDB item identifier.
//!
//! v0 only handles partition-key-only tables and stringifies the key.
//! Numeric keys round-trip through `String`; binary keys aren't covered
//! yet. Composite (partition + sort) keys arrive in a follow-up.

use std::fmt;
use std::str::FromStr;

use serde::{Serialize, Serializer};

use super::types::AttributeValue;

/// A DynamoDB primary key. v0 = partition-key-only as a string.
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct DynamoId(String);

impl DynamoId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Best-effort conversion from a wire `AttributeValue`. Returns
    /// `None` for variants that don't map to a primary-key shape.
    pub fn from_attr(value: &AttributeValue) -> Option<Self> {
        match value {
            AttributeValue::S(s) => Some(Self(s.clone())),
            AttributeValue::N(n) => Some(Self(n.clone())),
            _ => None,
        }
    }

    /// Wire representation. v0 picks `S` since the underlying storage is
    /// already a string; numeric keys would need explicit caller intent.
    pub fn to_attr(&self) -> AttributeValue {
        AttributeValue::S(self.0.clone())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for DynamoId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for DynamoId {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self(s.to_string()))
    }
}

impl Serialize for DynamoId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl From<String> for DynamoId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for DynamoId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}
