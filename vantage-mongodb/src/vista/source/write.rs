//! Building the BSON record a vista write hands to the table: spec columns
//! land at their BSON paths, and each value is typed by its declared column
//! type or by the row as stored (see the `coerce` module for the rules).

use bson::{Bson, Document};
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_table::table::Table;
use vantage_table::traits::column_like::ColumnLike;
use vantage_types::{EmptyEntity, Record};

use super::coerce::{self, BsonKind};
use super::{MongoTableShell, walk_bson_path};
use crate::id::{MongoId, id_variants};
use crate::mongodb::MongoDB;
use crate::types::{AnyMongoType, MongoType};
use crate::vista::cbor::{bson_to_cbor, cbor_to_bson};

/// The table's columns whose Rust type is a BSON type the CBOR surface reads
/// as text. The id column is left out: the id travels apart from the record
/// and is parsed by `parse_id`, as is an `_id` the record itself carries.
pub(super) fn declared_kinds(
    table: &Table<MongoDB, EmptyEntity>,
    id_column: Option<&str>,
) -> IndexMap<String, BsonKind> {
    table
        .columns()
        .iter()
        .filter(|(name, _)| id_column != Some(name.as_str()))
        .filter_map(|(name, col)| {
            BsonKind::of_column_type(col.get_type()).map(|kind| (name.clone(), kind))
        })
        .collect()
}

impl MongoTableShell {
    /// Build a `Record<AnyMongoType>` ready for the table's write path. For
    /// columns with a multi-segment path, intermediate sub-documents are
    /// reconstructed and merged so that `{ "address.city", "address.zip" }`
    /// land as one `address: { city, zip }` BSON entry. `stored` is the row
    /// as it stands, on replace and patch.
    pub(super) fn flatten_for_write(
        &self,
        record: &Record<CborValue>,
        stored: Option<&IndexMap<String, Bson>>,
    ) -> Result<Record<AnyMongoType>> {
        let mut flat = self.flatten_paths(record, stored)?;
        // CBOR carries an ObjectId as its hex text, so a value read from the
        // set arrives in the other spelling than the set's invariant, and the
        // table's conflict check compares raw values.
        for (column, invariant) in self.table.invariants() {
            let Some(outgoing) = flat.get(column).map(|v| v.to_bson()) else {
                continue;
            };
            if id_variants(&outgoing) == Some(invariant.to_bson()) {
                flat.insert(column.clone(), invariant.clone());
            }
        }
        Ok(flat)
    }

    fn flatten_paths(
        &self,
        record: &Record<CborValue>,
        stored: Option<&IndexMap<String, Bson>>,
    ) -> Result<Record<AnyMongoType>> {
        let mut top = Document::new();
        for (col_name, value) in record {
            let fallback;
            let path: &[String] = match self.column_paths.get(col_name) {
                Some(p) => p.as_slice(),
                None => {
                    fallback = vec![col_name.clone()];
                    &fallback
                }
            };
            let old = stored.and_then(|row| walk_bson_path(row, path));
            let bson_value = match (old, self.declared.get(col_name)) {
                (Some(old), _) if bson_to_cbor(&old) == *value => old,
                // A row copied from a read carries its `_id` as hex text; it
                // is stored the way the id itself is parsed, as an ObjectId.
                _ if path == ["_id"] => match value {
                    CborValue::Text(text) => self.parse_id(text).to_bson(),
                    other => cbor_to_bson(other),
                },
                (old, Some(kind)) => coerce::declared_over(col_name, *kind, old.as_ref(), value)?,
                (Some(old), None) => coerce::preserve(&old, value),
                (None, None) => cbor_to_bson(value),
            };
            if self.column_paths.is_empty() {
                top.insert(col_name.clone(), bson_value);
            } else {
                insert_at_path(&mut top, path, bson_value);
            }
        }
        Ok(top
            .into_iter()
            .map(|(k, v)| (k, AnyMongoType::untyped(v)))
            .collect())
    }

    /// The row as stored, which `flatten_for_write` types a replace or patch
    /// by; `None` when the set does not hold `id`.
    pub(super) async fn stored_row(&self, id: &MongoId) -> Result<Option<IndexMap<String, Bson>>> {
        Ok(self
            .table
            .get_value(id.clone())
            .await?
            .map(|row| row.into_iter().map(|(k, v)| (k, v.into_value())).collect()))
    }
}

/// Insert `value` at `path` inside `doc`, creating intermediate sub-documents
/// as needed. If a non-document scalar is in the way, it gets overwritten.
fn insert_at_path(doc: &mut Document, path: &[String], value: Bson) {
    if path.is_empty() {
        return;
    }
    if path.len() == 1 {
        doc.insert(path[0].clone(), value);
        return;
    }
    let head = path[0].clone();
    let entry = doc
        .entry(head)
        .or_insert_with(|| Bson::Document(Document::new()));
    if !matches!(entry, Bson::Document(_)) {
        *entry = Bson::Document(Document::new());
    }
    if let Bson::Document(sub) = entry {
        insert_at_path(sub, &path[1..], value);
    }
}

#[cfg(test)]
mod tests;
