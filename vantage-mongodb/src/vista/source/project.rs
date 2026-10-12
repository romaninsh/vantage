//! Reading spec columns out of raw documents along their BSON paths.

use bson::Bson;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_types::Record;

use crate::types::AnyMongoType;
use crate::vista::cbor::bson_to_cbor;

/// Project a raw document onto the spec columns named in `column_paths`.
/// Identity (top-level keys → CBOR) when no paths are configured.
pub(super) fn project_record(
    column_paths: &IndexMap<String, Vec<String>>,
    record: Record<AnyMongoType>,
) -> Record<CborValue> {
    if column_paths.is_empty() {
        return record
            .into_iter()
            .map(|(k, v)| (k, bson_to_cbor(v.value())))
            .collect();
    }
    let raw: IndexMap<String, Bson> = record
        .into_iter()
        .map(|(k, v)| (k, v.into_value()))
        .collect();
    let mut out = Record::new();
    for (spec_name, path) in column_paths {
        if let Some(value) = walk_bson_path(&raw, path) {
            out.insert(spec_name.clone(), bson_to_cbor(&value));
        }
    }
    out
}

/// Walk a dotted BSON path against a top-level map. Returns `None` if any
/// intermediate segment is missing or isn't a `Document`.
pub(super) fn walk_bson_path(map: &IndexMap<String, Bson>, path: &[String]) -> Option<Bson> {
    let head = path.first()?;
    let mut current = map.get(head)?.clone();
    for segment in &path[1..] {
        current = match current {
            Bson::Document(doc) => doc.get(segment)?.clone(),
            _ => return None,
        };
    }
    Some(current)
}
