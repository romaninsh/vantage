//! `serde_json::Value` (YAML `json`) → SQLite TEXT affinity, stored as JSON
//! text. Only a map or an array is a `json` value. The value stays a map or
//! an array until the binder writes its text, so a `json` field of a
//! contained record nests in the host's JSON rather than as a string.

use super::{SqliteType, SqliteTypeTextMarker};
use crate::types::{json_document, json_to_cbor};
use ciborium::Value;

impl SqliteType for serde_json::Value {
    type Target = SqliteTypeTextMarker;

    fn to_cbor(&self) -> Value {
        json_to_cbor(self.clone())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        json_document(value)
    }
}
