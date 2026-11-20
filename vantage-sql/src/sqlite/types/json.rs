//! `serde_json::Value` (YAML `json`) → SQLite TEXT affinity, stored as JSON
//! text. Only a map or an array is a `json` value.

use super::{SqliteType, SqliteTypeTextMarker};
use crate::types::json_document;
use ciborium::Value;

impl SqliteType for serde_json::Value {
    type Target = SqliteTypeTextMarker;

    fn to_cbor(&self) -> Value {
        Value::Text(self.to_string())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        json_document(value)
    }
}
