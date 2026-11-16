//! Reading spec columns out of raw documents along their BSON paths.

use bson::Bson;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

use super::coerce::{self, BsonKind};
use crate::types::AnyMongoType;

/// Project a raw document of `collection` onto the spec columns named in
/// `column_paths`. Identity (top-level keys → CBOR) when no paths are
/// configured. The `date` and `timestamp` columns in `declared` read typed;
/// one holding something that is no date is an error (see `coerce::read`).
pub(super) fn project_record(
    collection: &str,
    column_paths: &IndexMap<String, Vec<String>>,
    declared: &IndexMap<String, BsonKind>,
    record: Record<AnyMongoType>,
) -> Result<Record<CborValue>> {
    let cell = |column: &str, value: &Bson| {
        coerce::read(collection, column, declared.get(column).copied(), value)
    };
    if column_paths.is_empty() {
        return record
            .into_iter()
            .map(|(k, v)| {
                let value = cell(&k, v.value())?;
                Ok((k, value))
            })
            .collect();
    }
    let raw: IndexMap<String, Bson> = record
        .into_iter()
        .map(|(k, v)| (k, v.into_value()))
        .collect();
    let mut out = Record::new();
    for (spec_name, path) in column_paths {
        if let Some(value) = walk_bson_path(&raw, path) {
            out.insert(spec_name.clone(), cell(spec_name, &value)?);
        }
    }
    Ok(out)
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
