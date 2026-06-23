//! In-memory `TableShell` for tests and examples.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};
use vantage_core::Result;
use vantage_types::Record;

use crate::{
    build_contained_vista,
    capabilities::VistaCapabilities,
    column::Column,
    contained::ContainedWriteback,
    metadata::VistaMetadata,
    reference::{ContainedSpec, Reference},
    sort::SortDirection,
    source::TableShell,
    vista::Vista,
};

#[derive(Clone)]
pub struct MockShell {
    data: Arc<Mutex<IndexMap<String, Record<CborValue>>>>,
    next_auto_id: Arc<Mutex<i64>>,
    filters: Arc<Mutex<Vec<(String, CborValue)>>>,
    order: Arc<Mutex<Option<(String, SortDirection)>>>,
    search: Arc<Mutex<Option<String>>>,
    capabilities: VistaCapabilities,
    metadata: VistaMetadata,
}

impl MockShell {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(IndexMap::new())),
            next_auto_id: Arc::new(Mutex::new(1)),
            filters: Arc::new(Mutex::new(Vec::new())),
            order: Arc::new(Mutex::new(None)),
            search: Arc::new(Mutex::new(None)),
            capabilities: VistaCapabilities {
                can_count: true,
                can_insert: true,
                can_update: true,
                can_delete: true,
                can_order: true,
                can_search: true,
                ..VistaCapabilities::default()
            },
            metadata: VistaMetadata::new(),
        }
    }

    pub fn with_capabilities(mut self, capabilities: VistaCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    pub fn with_metadata(mut self, metadata: VistaMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Seed a record with an explicit id.
    pub fn with_record(self, id: impl Into<String>, record: Record<CborValue>) -> Self {
        self.data.lock().unwrap().insert(id.into(), record);
        self
    }

    // ---- Live dataset mutation ---------------------------------------------
    //
    // The store is `Arc<Mutex<…>>`, so a clone of this shell taken *before*
    // it is boxed into a `Vista` keeps a handle to the same rows. These
    // by-ref helpers let a test or example mutate the dataset mid-run —
    // simulating an upstream that changed between reads — and have the next
    // `list`/`get`/refresh observe it. They are additive and opt-in; an
    // untouched shell behaves exactly as before.

    /// Insert or replace a record by id through the shared store.
    pub fn set_record(&self, id: impl Into<String>, record: Record<CborValue>) {
        self.data.lock().unwrap().insert(id.into(), record);
    }

    /// Overwrite a single field of an existing record (read-modify-write).
    /// No-op if the record is absent.
    pub fn set_field(&self, id: &str, field: &str, value: CborValue) {
        if let Some(rec) = self.data.lock().unwrap().get_mut(id) {
            rec.insert(field.to_string(), value);
        }
    }

    /// Remove a record by id. No-op if absent.
    pub fn remove_record(&self, id: &str) {
        self.data.lock().unwrap().shift_remove(id);
    }

    /// Drop every record.
    pub fn clear_records(&self) {
        self.data.lock().unwrap().clear();
    }

    fn matches_filters(&self, record: &Record<CborValue>) -> bool {
        self.filters
            .lock()
            .unwrap()
            .iter()
            .all(|(field, expected)| record.get(field) == Some(expected))
    }

    fn matches_search(&self, record: &Record<CborValue>) -> bool {
        let guard = self.search.lock().unwrap();
        let Some(needle) = guard.as_deref() else {
            return true;
        };
        let needle_lc = needle.to_lowercase();
        record.values().any(|v| match v {
            CborValue::Text(s) => s.to_lowercase().contains(&needle_lc),
            _ => false,
        })
    }

    fn next_auto_id(&self) -> String {
        let mut next = self.next_auto_id.lock().unwrap();
        let id = next.to_string();
        *next += 1;
        id
    }
}

impl Default for MockShell {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TableShell for MockShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        &self.metadata.columns
    }

    fn references(&self) -> &IndexMap<String, Reference> {
        &self.metadata.references
    }

    fn contained(&self) -> &IndexMap<String, ContainedSpec> {
        &self.metadata.contained
    }

    /// Resolve a contained relation against `row`, with a writeback that patches
    /// the parent record's host column directly in this mock's store — the
    /// in-memory analogue of a driver patching its row.
    fn get_contained_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Vista> {
        let spec = self.metadata.contained.get(relation).ok_or_else(|| {
            vantage_core::error!("unknown contained relation", relation = relation)
        })?;
        let host_value = row.get(&spec.host_column).cloned();

        let id_field = self.metadata.id_column.as_deref().unwrap_or("id");
        let parent_id = match row.get(id_field) {
            Some(CborValue::Text(s)) => s.clone(),
            _ => {
                return Err(vantage_core::error!(
                    "contained traversal requires the parent row's id",
                    relation = relation
                ));
            }
        };

        let data = self.data.clone();
        let host_column = spec.host_column.clone();
        let writeback: ContainedWriteback = Arc::new(move |collection: CborValue| {
            let data = data.clone();
            let host_column = host_column.clone();
            let parent_id = parent_id.clone();
            Box::pin(async move {
                let mut store = data.lock().unwrap();
                if let Some(record) = store.get_mut(&parent_id) {
                    record.insert(host_column, collection);
                }
                Ok(())
            })
        });

        build_contained_vista(spec, host_value.as_ref(), writeback, None)
    }

    fn id_column(&self) -> Option<&str> {
        self.metadata.id_column.as_deref()
    }

    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        let data = self.data.lock().unwrap();
        let mut rows: Vec<(String, Record<CborValue>)> = data
            .iter()
            .filter(|(_, record)| self.matches_filters(record) && self.matches_search(record))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        if let Some((field, dir)) = self.order.lock().unwrap().clone() {
            rows.sort_by(|a, b| {
                let lhs = a.1.get(&field);
                let rhs = b.1.get(&field);
                let ord = cbor_cmp(lhs, rhs);
                match dir {
                    SortDirection::Ascending => ord,
                    SortDirection::Descending => ord.reverse(),
                }
            });
        }
        Ok(rows.into_iter().collect())
    }

    async fn get_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        Ok(self.data.lock().unwrap().get(id).cloned())
    }

    async fn get_vista_some_value(
        &self,
        _vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        let data = self.data.lock().unwrap();
        Ok(data
            .iter()
            .find(|(_, record)| self.matches_filters(record))
            .map(|(k, v)| (k.clone(), v.clone())))
    }

    async fn insert_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let mut data = self.data.lock().unwrap();
        if data.contains_key(id) {
            return Err(vantage_core::error!("Record already exists", id = id));
        }
        let mut stored = record.clone();
        stored.insert("id".to_string(), CborValue::Text(id.clone()));
        data.insert(id.clone(), stored.clone());
        Ok(stored)
    }

    async fn replace_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let mut data = self.data.lock().unwrap();
        let mut stored = record.clone();
        stored.insert("id".to_string(), CborValue::Text(id.clone()));
        data.insert(id.clone(), stored.clone());
        Ok(stored)
    }

    async fn patch_vista_value(
        &self,
        _vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        let mut data = self.data.lock().unwrap();
        let existing = data
            .get_mut(id)
            .ok_or_else(|| vantage_core::error!("Record not found", id = id))?;
        for (k, v) in partial {
            existing.insert(k.clone(), v.clone());
        }
        Ok(existing.clone())
    }

    async fn delete_vista_value(&self, _vista: &Vista, id: &String) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        if data.shift_remove(id).is_none() {
            Err(vantage_core::error!("Record not found", id = id))
        } else {
            Ok(())
        }
    }

    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        self.data.lock().unwrap().clear();
        Ok(())
    }

    async fn insert_vista_return_id_value(
        &self,
        vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        let id = match record.get("id") {
            Some(CborValue::Text(s)) if !s.is_empty() => s.clone(),
            Some(CborValue::Integer(i)) => i128::from(*i).to_string(),
            _ => self.next_auto_id(),
        };
        self.insert_vista_value(vista, &id, record).await?;
        Ok(id)
    }

    async fn get_vista_count(&self, vista: &Vista) -> Result<i64> {
        Ok(self.list_vista_values(vista).await?.len() as i64)
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "mock"
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        self.filters
            .lock()
            .unwrap()
            .push((field.to_string(), value.clone()));
        Ok(())
    }

    fn add_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        *self.order.lock().unwrap() = Some((field.to_string(), dir));
        Ok(())
    }

    fn clear_orders(&mut self) -> Result<()> {
        *self.order.lock().unwrap() = None;
        Ok(())
    }

    fn add_search(&mut self, text: &str) -> Result<()> {
        *self.search.lock().unwrap() = Some(text.to_string());
        Ok(())
    }

    fn clear_search(&mut self) -> Result<()> {
        *self.search.lock().unwrap() = None;
        Ok(())
    }
}

/// Total-order comparator for the CBOR scalars MockShell records carry.
/// Falls back to lexical ordering of CBOR-as-text for mixed-or-unknown
/// types, which keeps sort deterministic without claiming semantic
/// equivalence between heterogeneous values.
fn cbor_cmp(a: Option<&CborValue>, b: Option<&CborValue>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, _) => Ordering::Less,
        (_, None) => Ordering::Greater,
        (Some(lhs), Some(rhs)) => match (lhs, rhs) {
            (CborValue::Text(l), CborValue::Text(r)) => l.cmp(r),
            (CborValue::Integer(l), CborValue::Integer(r)) => i128::from(*l).cmp(&i128::from(*r)),
            (CborValue::Bool(l), CborValue::Bool(r)) => l.cmp(r),
            _ => format!("{lhs:?}").cmp(&format!("{rhs:?}")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Column, Reference, ReferenceKind, Vista, VistaMetadata};
    use vantage_dataset::{InsertableValueSet, ReadableValueSet, WritableValueSet};

    fn cbor_text(s: &str) -> CborValue {
        CborValue::Text(s.into())
    }

    fn record(pairs: &[(&str, CborValue)]) -> Record<CborValue> {
        let mut r = Record::new();
        for (k, v) in pairs {
            r.insert((*k).to_string(), v.clone());
        }
        r
    }

    fn build_user_vista(source: MockShell) -> Vista {
        let metadata = VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("name", "String").with_flag("title"))
            .with_column(Column::new("email", "String").hidden())
            .with_column(Column::new("vip_flag", "bool"))
            .with_id_column("id")
            .with_reference(Reference::new(
                "orders",
                "orders",
                ReferenceKind::HasMany,
                "user_id",
            ));
        Vista::new("users", Box::new(source.with_metadata(metadata)))
    }

    #[test]
    fn metadata_accessors_round_trip() {
        let vista = build_user_vista(MockShell::new());

        assert_eq!(vista.name(), "users");
        assert_eq!(vista.get_id_column(), Some("id"));
        assert_eq!(vista.get_title_columns(), vec!["name"]);
        assert_eq!(
            vista.get_column_names(),
            vec!["id", "name", "email", "vip_flag"]
        );
        assert!(vista.get_column("email").unwrap().is_hidden());
        assert!(!vista.get_column("name").unwrap().is_hidden());
        assert_eq!(vista.get_references(), vec!["orders".to_string()]);
        assert_eq!(
            vista.get_reference("orders").unwrap().foreign_key,
            "user_id"
        );

        let caps = vista.capabilities();
        assert!(caps.can_count && caps.can_insert && caps.can_update && caps.can_delete);
        assert!(!caps.can_subscribe);
    }

    #[tokio::test]
    async fn list_values_returns_seeded_rows() {
        let source = MockShell::new()
            .with_record(
                "1",
                record(&[("id", cbor_text("1")), ("name", cbor_text("Alice"))]),
            )
            .with_record(
                "2",
                record(&[("id", cbor_text("2")), ("name", cbor_text("Bob"))]),
            );
        let vista = build_user_vista(source);

        let rows = vista.list_values().await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.contains_key("1"));
        assert_eq!(rows["2"].get("name"), Some(&cbor_text("Bob")));

        let alice = vista.get_value("1").await.unwrap().unwrap();
        assert_eq!(alice.get("name"), Some(&cbor_text("Alice")));

        assert_eq!(vista.get_count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn add_condition_eq_filters_list_and_count() {
        let source = MockShell::new()
            .with_record(
                "1",
                record(&[
                    ("name", cbor_text("Alice")),
                    ("vip_flag", CborValue::Bool(true)),
                ]),
            )
            .with_record(
                "2",
                record(&[
                    ("name", cbor_text("Bob")),
                    ("vip_flag", CborValue::Bool(false)),
                ]),
            )
            .with_record(
                "3",
                record(&[
                    ("name", cbor_text("Carol")),
                    ("vip_flag", CborValue::Bool(true)),
                ]),
            );
        let mut vista = build_user_vista(source);
        vista
            .add_condition_eq("vip_flag", CborValue::Bool(true))
            .unwrap();

        let rows = vista.list_values().await.unwrap();
        assert_eq!(rows.len(), 2);
        assert!(rows.contains_key("1"));
        assert!(rows.contains_key("3"));
        assert_eq!(vista.get_count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn writable_value_set_round_trip() {
        let vista = build_user_vista(MockShell::new());

        // insert_value with explicit id
        let inserted = vista
            .insert_value("alice", &record(&[("name", cbor_text("Alice"))]))
            .await
            .unwrap();
        assert_eq!(inserted.get("id"), Some(&cbor_text("alice")));

        // duplicate insert_value fails
        let dup = vista.insert_value("alice", &record(&[])).await;
        assert!(dup.is_err());

        // replace_value upserts
        vista
            .replace_value("alice", &record(&[("name", cbor_text("Alicia"))]))
            .await
            .unwrap();
        let renamed = vista.get_value("alice").await.unwrap().unwrap();
        assert_eq!(renamed.get("name"), Some(&cbor_text("Alicia")));

        // patch_value merges
        vista
            .patch_value(
                "alice",
                &record(&[("email", cbor_text("alice@example.com"))]),
            )
            .await
            .unwrap();
        let patched = vista.get_value("alice").await.unwrap().unwrap();
        assert_eq!(patched.get("name"), Some(&cbor_text("Alicia")));
        assert_eq!(patched.get("email"), Some(&cbor_text("alice@example.com")));

        // delete
        vista.delete("alice").await.unwrap();
        assert!(vista.get_value("alice").await.unwrap().is_none());

        // delete_all
        vista
            .insert_value("a", &record(&[("name", cbor_text("A"))]))
            .await
            .unwrap();
        vista
            .insert_value("b", &record(&[("name", cbor_text("B"))]))
            .await
            .unwrap();
        vista.delete_all().await.unwrap();
        assert_eq!(vista.list_values().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn default_get_value_with_row_ignores_row_and_delegates() {
        // A driver that does not override `get_vista_value_with_row` must behave
        // exactly like `get_value` — the extra `row` is ignored.
        let source = MockShell::new().with_record(
            "x",
            record(&[("id", cbor_text("x")), ("name", cbor_text("Xavier"))]),
        );
        let vista = build_user_vista(source);

        let mut row: Record<CborValue> = Record::new();
        row.insert("extra".into(), cbor_text("ignored"));

        let got = vista.get_value_with_row("x", &row).await.unwrap().unwrap();
        assert_eq!(got.get("name"), Some(&cbor_text("Xavier")));
    }

    #[tokio::test]
    async fn insertable_value_set_assigns_ids() {
        let vista = build_user_vista(MockShell::new());

        // record without id → mock generates one
        let auto_id = vista
            .insert_return_id_value(&record(&[("name", cbor_text("Bob"))]))
            .await
            .unwrap();
        assert_eq!(auto_id, "1");

        // record with explicit string id → preserved
        let explicit = vista
            .insert_return_id_value(&record(&[
                ("id", cbor_text("alice")),
                ("name", cbor_text("Alice")),
            ]))
            .await
            .unwrap();
        assert_eq!(explicit, "alice");

        assert_eq!(vista.get_count().await.unwrap(), 2);
    }
}
