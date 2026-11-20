//! `serde_json::Value` (YAML `json`) → MySQL, stored as JSON text, which a
//! `JSON` column and a text column both take. Only a map or an array is a
//! `json` value.

use super::{MysqlType, MysqlTypeTextMarker};
use crate::types::json_document;
use ciborium::Value;

impl MysqlType for serde_json::Value {
    type Target = MysqlTypeTextMarker;

    fn to_cbor(&self) -> Value {
        Value::Text(self.to_string())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        json_document(value)
    }
}
