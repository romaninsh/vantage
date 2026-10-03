//! [`ExtraFields`]: undeclared filler columns riding on every generated row.

use ciborium::Value as CborValue;
use serde::Deserialize;
use vantage_types::Record;

/// Undeclared payload riding along on every record: `count` extra fields of
/// `size`-char strings — the fat API response the query didn't ask for.
///
/// Field `i` (from 1) is named `extra_{i:04}` and holds `"{id}:{i}:"` padded
/// with `x` (or cut) to exactly `size` bytes, so every cell is distinct and
/// reproducible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExtraFields {
    pub count: usize,
    pub size: usize,
}

impl ExtraFields {
    /// Append this filler to `record`, the row stored under `id`.
    pub(super) fn apply(&self, id: &str, record: &mut Record<CborValue>) {
        for i in 1..=self.count {
            let mut s = format!("{id}:{i}:");
            while s.len() < self.size {
                s.push('x');
            }
            s.truncate(self.size);
            record.insert(format!("extra_{i:04}"), CborValue::Text(s));
        }
    }
}
