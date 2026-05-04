//! `MongoTableShell` — owns the typed `Table<MongoDB, EmptyEntity>` and
//! exposes it through the `TableShell` boundary.
//!
//! Each spec column carries a BSON path (`column_paths`) which the source uses
//! to walk nested documents on read, reconstruct nested documents on write,
//! and translate `eq` filters into Mongo's dot-notation form. When the path
//! map is empty (e.g. a typed `from_table` with no columns), the source falls
//! back to identity passthrough on top-level keys.

use std::str::FromStr;

use async_trait::async_trait;
use bson::{Bson, Document, doc};
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use vantage_vista::{TableShell, Vista, VistaCapabilities};

use crate::id::MongoId;
use crate::mongodb::MongoDB;
use crate::types::AnyMongoType;
use crate::vista::cbor::{bson_to_cbor, cbor_to_bson};

pub struct MongoTableShell {
    pub(crate) table: Table<MongoDB, EmptyEntity>,
    pub(crate) capabilities: VistaCapabilities,
    /// spec column name → BSON path (e.g. `"city"` → `["address", "city"]`).
    /// Empty map ⇒ identity passthrough (used by typed `from_table` paths).
    pub(crate) column_paths: IndexMap<String, Vec<String>>,
}

impl MongoTableShell {
    pub(crate) fn new(
        table: Table<MongoDB, EmptyEntity>,
        capabilities: VistaCapabilities,
        column_paths: IndexMap<String, Vec<String>>,
    ) -> Self {
        Self {
            table,
            capabilities,
            column_paths,
        }
    }

    pub(crate) fn parse_id(&self, id: &str) -> MongoId {
        MongoId::from_str(id).unwrap_or_else(|_| MongoId::String(id.to_string()))
    }

    async fn read_all(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        let raw = self.table.list_values().await?;
        Ok(raw
            .into_iter()
            .map(|(id, record)| (id.to_string(), self.unflatten_to_cbor(record)))
            .collect())
    }

    /// Walk `column_paths` and project each spec column out of the raw doc.
    /// Identity (top-level keys → CBOR) when no paths are configured.
    fn unflatten_to_cbor(&self, record: Record<AnyMongoType>) -> Record<CborValue> {
        if self.column_paths.is_empty() {
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
        for (spec_name, path) in &self.column_paths {
            if let Some(value) = walk_bson_path(&raw, path) {
                out.insert(spec_name.clone(), bson_to_cbor(&value));
            }
        }
        out
    }

    /// Build a `Record<AnyMongoType>` ready for the table's write path. For
    /// columns with a multi-segment path, intermediate sub-documents are
    /// reconstructed and merged so that `{ "address.city", "address.zip" }`
    /// land as one `address: { city, zip }` BSON entry.
    fn flatten_for_write(&self, record: &Record<CborValue>) -> Record<AnyMongoType> {
        if self.column_paths.is_empty() {
            return record
                .iter()
                .map(|(k, v)| (k.clone(), AnyMongoType::untyped(cbor_to_bson(v))))
                .collect();
        }
        let mut top = Document::new();
        for (col_name, value) in record {
            let bson_value = cbor_to_bson(value);
            let fallback;
            let path: &[String] = match self.column_paths.get(col_name) {
                Some(p) => p.as_slice(),
                None => {
                    fallback = vec![col_name.clone()];
                    &fallback
                }
            };
            insert_at_path(&mut top, path, bson_value);
        }
        top.into_iter()
            .map(|(k, v)| (k, AnyMongoType::untyped(v)))
            .collect()
    }

    /// Dotted form of a spec column's BSON path, used for Mongo filters.
    /// Falls back to the spec name when the column isn't in the path map.
    fn dotted_path(&self, field: &str) -> String {
        match self.column_paths.get(field) {
            Some(path) => path.join("."),
            None => field.to_string(),
        }
    }
}

/// Walk a dotted BSON path against a top-level map. Returns `None` if any
/// intermediate segment is missing or isn't a `Document`.
fn walk_bson_path(map: &IndexMap<String, Bson>, path: &[String]) -> Option<Bson> {
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

#[async_trait]
impl TableShell for MongoTableShell {
    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        self.read_all().await
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        let mongo_id = self.parse_id(id);
        let Some(record) = self.table.get_value(&mongo_id).await? else {
            return Ok(None);
        };
        Ok(Some(self.unflatten_to_cbor(record)))
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let data = self.read_all().await?;
        Ok(data.into_iter().next())
    }

    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        self.table.get_count().await
    }

    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let mongo_id = self.parse_id(id);
        let mongo_record = self.flatten_for_write(record);
        let inserted = self.table.insert_value(&mongo_id, &mongo_record).await?;
        Ok(self.unflatten_to_cbor(inserted))
    }

    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let mongo_id = self.parse_id(id);
        let mongo_record = self.flatten_for_write(record);
        let replaced = self.table.replace_value(&mongo_id, &mongo_record).await?;
        Ok(self.unflatten_to_cbor(replaced))
    }

    async fn patch_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let mongo_id = self.parse_id(id);
        let mongo_partial = self.flatten_for_write(partial);
        let patched = self.table.patch_value(&mongo_id, &mongo_partial).await?;
        Ok(self.unflatten_to_cbor(patched))
    }

    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        let mongo_id = self.parse_id(id);
        self.table.delete(&mongo_id).await
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        self.table.delete_all().await
    }

    async fn insert_vista_return_id_value(
        &self,
        _vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        let mongo_record = self.flatten_for_write(record);
        let id = self.table.insert_return_id_value(&mongo_record).await?;
        Ok(id.to_string())
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        let dotted = self.dotted_path(field);
        let bson_value = cbor_to_bson(value);
        let filter = doc! { dotted: bson_value };
        self.table.add_condition(filter);
        Ok(())
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn walk_bson_path_returns_nested_value() {
        let mut inner = Document::new();
        inner.insert("city", "Hill Valley");
        let mut outer = IndexMap::new();
        outer.insert("address".to_string(), Bson::Document(inner));

        let value = walk_bson_path(&outer, &["address".to_string(), "city".to_string()]);
        assert_eq!(value, Some(Bson::String("Hill Valley".to_string())));
    }

    #[test]
    fn walk_bson_path_missing_segment_yields_none() {
        let mut outer = IndexMap::new();
        outer.insert("address".to_string(), Bson::Document(Document::new()));

        let value = walk_bson_path(&outer, &["address".to_string(), "city".to_string()]);
        assert_eq!(value, None);
    }

    #[test]
    fn walk_bson_path_through_scalar_yields_none() {
        let mut outer = IndexMap::new();
        outer.insert("address".to_string(), Bson::String("123 Main".into()));

        let value = walk_bson_path(&outer, &["address".to_string(), "city".to_string()]);
        assert_eq!(value, None);
    }

    #[test]
    fn insert_at_path_builds_nested_doc() {
        let mut doc = Document::new();
        insert_at_path(
            &mut doc,
            &["address".to_string(), "city".to_string()],
            Bson::String("NYC".into()),
        );
        let address = doc.get_document("address").unwrap();
        assert_eq!(address.get_str("city").unwrap(), "NYC");
    }

    #[test]
    fn insert_at_path_merges_siblings() {
        let mut doc = Document::new();
        insert_at_path(
            &mut doc,
            &["address".to_string(), "city".to_string()],
            Bson::String("NYC".into()),
        );
        insert_at_path(
            &mut doc,
            &["address".to_string(), "zip".to_string()],
            Bson::String("10001".into()),
        );
        let address = doc.get_document("address").unwrap();
        assert_eq!(address.get_str("city").unwrap(), "NYC");
        assert_eq!(address.get_str("zip").unwrap(), "10001");
    }

    #[test]
    fn insert_at_path_overwrites_scalar_with_nested() {
        let mut doc = Document::new();
        doc.insert("address", "literal");
        insert_at_path(
            &mut doc,
            &["address".to_string(), "city".to_string()],
            Bson::String("NYC".into()),
        );
        let address = doc.get_document("address").unwrap();
        assert_eq!(address.get_str("city").unwrap(), "NYC");
    }
}
