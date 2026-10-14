//! The [`TableShell`] behaviour of [`MockShell`]. Search, order and
//! traversal answer `Unsupported` unless the mock's capabilities include
//! them, so a test can build a refusing mock with `with_capabilities`.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};

use super::{MockShell, Rec};
use crate::{
    capabilities::{Capability, VistaCapabilities, unknown_relation, unsupported},
    metadata::VistaMetadata,
    page::{Page, Preview},
    query::Condition,
    refuse::condition_column_error,
    shell::TableShell,
    sort::SortDirection,
    vista::Vista,
};

impl MockShell {
    fn require(&self, capability: Capability) -> Result<()> {
        if self.capabilities.has(capability) {
            Ok(())
        } else {
            unsupported(capability)
        }
    }
}

#[async_trait]
impl TableShell for MockShell {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
    fn driver_name(&self) -> &'static str {
        "mock"
    }
    /// The mock has no query language, so it reports its narrowing.
    fn preview(&self) -> Preview {
        Preview::new().with_query(self.render_query())
    }

    async fn list(&self) -> Result<IndexMap<String, Rec>> {
        self.guard_reads()?;
        self.gets.refuse_listing("list")?;
        let data = self.data.lock().unwrap();
        Ok(self
            .matching(&data)?
            .into_iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    async fn get(&self, id: &str) -> Result<Option<Rec>> {
        self.guard_reads()?;
        self.gets.admit(id).await?;
        let data = self.data.lock().unwrap();
        Ok(self.held(&data, &self.key(id))?.cloned())
    }

    /// Counts the matching rows without copying them.
    async fn count(&self) -> Result<i64> {
        self.guard_reads()?;
        self.gets.refuse_listing("count")?;
        let data = self.data.lock().unwrap();
        Ok(self.matching(&data)?.len() as i64)
    }

    /// Filters and orders references, then copies only the window: copying
    /// the whole store per window stalls on a large mock.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        self.guard_reads()?;
        self.gets.refuse_listing("fetch_window")?;
        let data = self.data.lock().unwrap();
        let rows = self.matching(&data)?;
        let total = rows.len() as i64;
        let window = rows
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        Ok(Page::new(window).with_total(total))
    }

    /// Conditions evaluate against a row's own cells. A dotted path into a
    /// declared column is refused with
    /// [`condition_column_error`], so a caller can
    /// evaluate it over the rows it reads; any other undeclared column is a
    /// plain error. A mock that declares no columns takes any flat column,
    /// as a schemaless store would, and refuses every dotted one.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        let column = condition.column.as_str();
        let columns = &self.metadata.columns;
        let schemaless_flat = columns.is_empty() && !column.contains('.');
        if !(schemaless_flat || columns.contains_key(column)) {
            return Err(condition_column_error(&self.metadata, column));
        }
        self.query.conditions.push(condition.clone());
        Ok(())
    }

    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.require(Capability::Search)?;
        self.query.search = text.map(str::to_string);
        Ok(())
    }

    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        self.require(Capability::Order)?;
        self.query.order = order.map(|(column, dir)| (column.to_string(), dir));
        Ok(())
    }

    /// Stores the record and reports it back carrying its key in the id
    /// column the metadata declares. Without an id, the key is the record's
    /// own `id` field, else the next auto id.
    async fn insert(&self, id: Option<&str>, record: &Rec) -> Result<(String, Rec)> {
        let id = match id {
            Some(id) => id.to_string(),
            None => match record.get("id") {
                Some(CborValue::Text(s)) if !s.is_empty() => s.clone(),
                Some(CborValue::Integer(i)) => i128::from(*i).to_string(),
                _ => self.next_auto_id(),
            },
        };
        let key = self.key(&id);
        let mut data = self.data.lock().unwrap();
        if let Some(existing) = data.get(&key) {
            if self.in_set(existing)? {
                return Ok((key, existing.clone()));
            }
            return Err(error!("Record is held outside the set", id = id).mark_conflict());
        }
        let stored = self.fit(record, &key)?;
        data.insert(key.clone(), stored.clone());
        Ok((key, stored))
    }

    async fn replace(&self, id: &str, record: &Rec) -> Result<Rec> {
        let key = self.key(id);
        let mut data = self.data.lock().unwrap();
        if data.contains_key(&key) && self.held(&data, &key)?.is_none() {
            return Err(error!("Record is held outside the set", id = id).mark_conflict());
        }
        let stored = self.fit(record, &key)?;
        data.insert(key, stored.clone());
        Ok(stored)
    }

    /// The mock's replace creates a missing row, so it is the upsert too.
    async fn upsert(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.replace(id, record).await
    }

    async fn patch(&self, id: &str, partial: &Rec) -> Result<Rec> {
        let key = self.key(id);
        let mut data = self.data.lock().unwrap();
        let mut patched = self
            .held(&data, &key)?
            .cloned()
            .ok_or_else(|| error!("Record not found", id = id).mark_not_found())?;
        for (k, v) in partial {
            patched.insert(k.clone(), v.clone());
        }
        if !self.in_set(&patched)? {
            return Err(
                error!("Patch would move the record out of the set", id = id).mark_conflict(),
            );
        }
        data.insert(key, patched.clone());
        Ok(patched)
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let key = self.key(id);
        let mut data = self.data.lock().unwrap();
        if self.held(&data, &key)?.is_some() {
            data.shift_remove(&key);
        }
        Ok(())
    }

    async fn delete_all(&self) -> Result<()> {
        let mut data = self.data.lock().unwrap();
        let doomed: Vec<String> = self
            .matching(&data)?
            .into_iter()
            .map(|(id, _)| id.clone())
            .collect();
        for id in doomed {
            data.shift_remove(&id);
        }
        Ok(())
    }

    fn get_ref(&self, relation: &str, row: Option<&Rec>) -> Result<Vista> {
        self.require(Capability::Traverse)?;
        let reference = self
            .metadata
            .references
            .get(relation)
            .ok_or_else(|| unknown_relation(relation))?;
        if reference.kind.is_contained() {
            self.contained_ref(reference, row)
        } else {
            self.join_ref(reference, row)
        }
    }
}
