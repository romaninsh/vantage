//! `serde_json::Value` (YAML `json`) → Postgres, bound as `jsonb`, which a
//! `jsonb`/`json` column takes and a text column stores as its JSON text.
//! Only a map or an array is a `json` value.

use super::{PostgresType, PostgresTypeJsonMarker};
use crate::types::{json_document, json_to_cbor};
use ciborium::Value;

impl PostgresType for serde_json::Value {
    type Target = PostgresTypeJsonMarker;

    fn to_cbor(&self) -> Value {
        json_to_cbor(self.clone())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        json_document(value)
    }
}
