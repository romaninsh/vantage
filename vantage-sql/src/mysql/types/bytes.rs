//! Byte strings → Mysql BLOB/BYTEA. Only CBOR bytes decode: text is
//! ambiguous (UTF-8 or base64?), so it isn't taken for bytes.

use super::{MysqlType, MysqlTypeBlobMarker};
use ciborium::Value;

impl MysqlType for Vec<u8> {
    type Target = MysqlTypeBlobMarker;

    fn to_cbor(&self) -> Value {
        Value::Bytes(self.clone())
    }

    fn from_cbor(value: Value) -> Option<Self> {
        match value {
            Value::Bytes(b) => Some(b),
            _ => None,
        }
    }
}
