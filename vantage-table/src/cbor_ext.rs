//! `serde_json::Value`-style accessors for `ciborium::Value`: the short names
//! (`as_str`, `as_i64`) and string-keyed map lookup that ciborium lacks.

use ciborium::Value as CborValue;

/// Short accessors and string-keyed map lookup for `ciborium::Value`.
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
