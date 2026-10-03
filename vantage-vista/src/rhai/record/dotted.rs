//! Dotted-key merge for `record.set(#{ "a.b": 1 })`: a key with dots writes
//! through to a nested CBOR map so a form field like `"inventory.stock"`
//! lands as the embedded object a schema expects, not a flat top-level key
//! with a literal `.` in its name.
//!
//! Ported from vantage-ui's `crates/actions/src/row/active.rs`, which keeps
//! the same semantics for its own row vocabulary.

use ciborium::Value as CborValue;
use indexmap::IndexMap;

/// Set `value` at `path` in `changes`, walking dotted segments and
/// materialising nested `CborValue::Map`s where needed. Existing nested
/// maps merge (siblings preserved); existing non-map values at a parent
/// path are replaced with a fresh map.
pub fn set_dotted(changes: &mut IndexMap<String, CborValue>, path: &str, value: CborValue) {
    let segments: Vec<&str> = path.split('.').collect();
    if segments.len() == 1 {
        changes.insert(path.to_string(), value);
        return;
    }
    let head = segments[0];
    let tail = &segments[1..];
    let entry = changes
        .entry(head.to_string())
        .or_insert_with(|| CborValue::Map(Vec::new()));
    if !matches!(entry, CborValue::Map(_)) {
        *entry = CborValue::Map(Vec::new());
    }
    set_dotted_cbor(entry, tail, value);
}

fn set_dotted_cbor(node: &mut CborValue, segments: &[&str], value: CborValue) {
    let CborValue::Map(pairs) = node else {
        return;
    };
    let head = segments[0];
    let tail = &segments[1..];
    let pos = pairs
        .iter()
        .position(|(k, _)| matches!(k, CborValue::Text(t) if t == head));
    if tail.is_empty() {
        match pos {
            Some(p) => pairs[p].1 = value,
            None => pairs.push((CborValue::Text(head.to_string()), value)),
        }
        return;
    }
    let idx = match pos {
        Some(p) => {
            if !matches!(pairs[p].1, CborValue::Map(_)) {
                pairs[p].1 = CborValue::Map(Vec::new());
            }
            p
        }
        None => {
            pairs.push((
                CborValue::Text(head.to_string()),
                CborValue::Map(Vec::new()),
            ));
            pairs.len() - 1
        }
    };
    set_dotted_cbor(&mut pairs[idx].1, tail, value);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_key_sets_directly() {
        let mut changes = IndexMap::new();
        set_dotted(&mut changes, "total", CborValue::Integer(5.into()));
        assert_eq!(changes.get("total"), Some(&CborValue::Integer(5.into())));
    }

    #[test]
    fn dotted_key_builds_nested_map() {
        let mut changes = IndexMap::new();
        set_dotted(
            &mut changes,
            "inventory.stock",
            CborValue::Integer(12.into()),
        );
        let CborValue::Map(pairs) = changes.get("inventory").unwrap() else {
            panic!("expected nested map");
        };
        assert_eq!(pairs.len(), 1);
    }

    #[test]
    fn sibling_dotted_keys_merge() {
        let mut changes = IndexMap::new();
        set_dotted(&mut changes, "meta.a", CborValue::Integer(1.into()));
        set_dotted(&mut changes, "meta.b", CborValue::Integer(2.into()));
        let CborValue::Map(pairs) = changes.get("meta").unwrap() else {
            panic!("expected nested map");
        };
        assert_eq!(pairs.len(), 2, "siblings must coexist after merge");
    }
}
