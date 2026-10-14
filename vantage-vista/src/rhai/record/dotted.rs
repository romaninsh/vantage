//! Dotted-key merge for `record.set(#{ "a.b": 1 })`: a key with dots writes
//! through to a nested CBOR map so a form field like `"inventory.stock"`
//! lands as the embedded object a schema expects, not a flat top-level key
//! with a literal `.` in its name.

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
