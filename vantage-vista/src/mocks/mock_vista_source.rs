//! In-memory `VistaSource` for tests and examples.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use std::sync::{Arc, Mutex};
use vantage_core::Result;
use vantage_types::Record;

use crate::{capabilities::VistaCapabilities, source::VistaSource, vista::Vista};

#[derive(Clone)]
pub struct MockVistaSource {
    data: Arc<Mutex<IndexMap<String, Record<CborValue>>>>,
    next_auto_id: Arc<Mutex<i64>>,
    filters: Arc<Mutex<Vec<(String, CborValue)>>>,
    capabilities: VistaCapabilities,
}

impl MockVistaSource {
    pub fn new() -> Self {
        Self {
            data: Arc::new(Mutex::new(IndexMap::new())),
            next_auto_id: Arc::new(Mutex::new(1)),
            filters: Arc::new(Mutex::new(Vec::new())),
            capabilities: VistaCapabilities {
                can_count: true,
                can_insert: true,
                can_update: true,
                can_delete: true,
                ..VistaCapabilities::default()
            },
        }
    }

    pub fn with_capabilities(mut self, capabilities: VistaCapabilities) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Seed a record with an explicit id.
    pub fn with_record(self, id: impl Into<String>, record: Record<CborValue>) -> Self {
        self.data.lock().unwrap().insert(id.into(), record);
        self
    }

    fn matches_filters(&self, record: &Record<CborValue>) -> bool {
        self.filters
            .lock()
            .unwrap()
            .iter()
            .all(|(field, expected)| record.get(field) == Some(expected))
    }

    fn next_auto_id(&self) -> String {
        let mut next = self.next_auto_id.lock().unwrap();
        let id = next.to_string();
        *next += 1;
        id
    }
}

impl Default for MockVistaSource {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VistaSource for MockVistaSource {
    async fn list_vista_values(
        &self,
        _vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        let data = self.data.lock().unwrap();
        Ok(data
            .iter()
            .filter(|(_, record)| self.matches_filters(record))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
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

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        self.filters
            .lock()
            .unwrap()
            .push((field.to_string(), value.clone()));
        Ok(())
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

    fn build_user_vista(source: MockVistaSource) -> Vista {
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
        Vista::new("users", Box::new(source), metadata)
    }

    #[test]
    fn metadata_accessors_round_trip() {
        let vista = build_user_vista(MockVistaSource::new());

        assert_eq!(vista.name(), "users");
        assert_eq!(vista.get_id_column(), Some("id"));
        assert_eq!(vista.get_title_columns(), vec!["name"]);
        assert_eq!(
            vista.get_column_names(),
            vec!["id", "name", "email", "vip_flag"]
        );
        assert!(vista.get_column("email").unwrap().is_hidden());
        assert!(!vista.get_column("name").unwrap().is_hidden());
        assert_eq!(vista.get_references(), vec!["orders"]);
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
        let source = MockVistaSource::new()
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

        let alice = vista.get_value(&"1".to_string()).await.unwrap().unwrap();
        assert_eq!(alice.get("name"), Some(&cbor_text("Alice")));

        assert_eq!(vista.get_count().await.unwrap(), 2);
    }

    #[tokio::test]
    async fn add_condition_eq_filters_list_and_count() {
        let source = MockVistaSource::new()
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
        let vista = build_user_vista(MockVistaSource::new());

        // insert_value with explicit id
        let inserted = vista
            .insert_value(
                &"alice".to_string(),
                &record(&[("name", cbor_text("Alice"))]),
            )
            .await
            .unwrap();
        assert_eq!(inserted.get("id"), Some(&cbor_text("alice")));

        // duplicate insert_value fails
        let dup = vista.insert_value(&"alice".to_string(), &record(&[])).await;
        assert!(dup.is_err());

        // replace_value upserts
        vista
            .replace_value(
                &"alice".to_string(),
                &record(&[("name", cbor_text("Alicia"))]),
            )
            .await
            .unwrap();
        let renamed = vista
            .get_value(&"alice".to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(renamed.get("name"), Some(&cbor_text("Alicia")));

        // patch_value merges
        vista
            .patch_value(
                &"alice".to_string(),
                &record(&[("email", cbor_text("alice@example.com"))]),
            )
            .await
            .unwrap();
        let patched = vista
            .get_value(&"alice".to_string())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(patched.get("name"), Some(&cbor_text("Alicia")));
        assert_eq!(patched.get("email"), Some(&cbor_text("alice@example.com")));

        // delete
        vista.delete(&"alice".to_string()).await.unwrap();
        assert!(
            vista
                .get_value(&"alice".to_string())
                .await
                .unwrap()
                .is_none()
        );

        // delete_all
        vista
            .insert_value(&"a".to_string(), &record(&[("name", cbor_text("A"))]))
            .await
            .unwrap();
        vista
            .insert_value(&"b".to_string(), &record(&[("name", cbor_text("B"))]))
            .await
            .unwrap();
        vista.delete_all().await.unwrap();
        assert_eq!(vista.list_values().await.unwrap().len(), 0);
    }

    #[tokio::test]
    async fn insertable_value_set_assigns_ids() {
        let vista = build_user_vista(MockVistaSource::new());

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
