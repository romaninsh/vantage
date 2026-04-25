//! Convenience accessors for `ciborium::Value` that mirror the muscle-memory
//! API of `serde_json::Value`.
//!
//! ciborium's native `Value` already provides `as_text`, `as_integer`,
//! `as_float`, `as_array`, `as_map`, `as_bytes` and friends. What it lacks —
//! and what code reading typed records reaches for constantly — are the
//! shorter names (`as_str`, `as_i64`) and string-keyed `Map` lookup. This
//! trait fills exactly that gap; nothing more.

use ciborium::Value as CborValue;

pub trait CborValueExt {
    /// Borrow the inner string if this is a `Text` value.
    fn as_str(&self) -> Option<&str>;

    /// Try to extract an `i64`. Returns `None` if the value is not an
    /// integer or doesn't fit.
    fn as_i64(&self) -> Option<i64>;

    /// Try to extract a `u64`. Returns `None` if the value is not an
    /// integer or doesn't fit.
    fn as_u64(&self) -> Option<u64>;

    /// Borrow the inner `f64` if this is a `Float` value.
    fn as_f64(&self) -> Option<f64>;

    /// Look up a value in a `Map` by string key. Returns `None` if the
    /// value isn't a map or the key isn't present.
    fn get(&self, key: &str) -> Option<&CborValue>;

    /// Same as [`get`](Self::get), but returns a mutable reference.
    fn get_mut(&mut self, key: &str) -> Option<&mut CborValue>;
}

impl CborValueExt for CborValue {
    fn as_str(&self) -> Option<&str> {
        self.as_text()
    }

    fn as_i64(&self) -> Option<i64> {
        self.as_integer().and_then(|i| i64::try_from(i).ok())
    }

    fn as_u64(&self) -> Option<u64> {
        self.as_integer().and_then(|i| u64::try_from(i).ok())
    }

    fn as_f64(&self) -> Option<f64> {
        self.as_float()
    }

    fn get(&self, key: &str) -> Option<&CborValue> {
        self.as_map()?
            .iter()
            .find(|(k, _)| k.as_text() == Some(key))
            .map(|(_, v)| v)
    }

    fn get_mut(&mut self, key: &str) -> Option<&mut CborValue> {
        self.as_map_mut()?
            .iter_mut()
            .find(|(k, _)| k.as_text() == Some(key))
            .map(|(_, v)| v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(pairs: Vec<(&str, CborValue)>) -> CborValue {
        CborValue::Map(
            pairs
                .into_iter()
                .map(|(k, v)| (CborValue::Text(k.to_string()), v))
                .collect(),
        )
    }

    #[test]
    fn as_str_and_int_accessors() {
        let v = CborValue::Text("hello".into());
        assert_eq!(v.as_str(), Some("hello"));
        assert_eq!(v.as_i64(), None);

        let n = CborValue::Integer(42i64.into());
        assert_eq!(n.as_i64(), Some(42));
        assert_eq!(n.as_u64(), Some(42));
        assert_eq!(n.as_str(), None);
    }

    #[test]
    fn map_lookup_by_str_key() {
        let mut record = map(vec![
            ("name", CborValue::Text("alice".into())),
            ("age", CborValue::Integer(30i64.into())),
        ]);
        assert_eq!(record.get("name").and_then(|v| v.as_str()), Some("alice"));
        assert_eq!(record.get("age").and_then(|v| v.as_i64()), Some(30));
        assert!(record.get("missing").is_none());

        // mutable lookup
        if let Some(age) = record.get_mut("age") {
            *age = CborValue::Integer(31i64.into());
        }
        assert_eq!(record.get("age").and_then(|v| v.as_i64()), Some(31));
    }
}
