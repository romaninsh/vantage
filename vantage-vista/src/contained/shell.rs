//! The [`TableShell`] behaviour of [`ContainedShell`].

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_types::Record;

use super::{ContainedShell, ONE_ID, cbor_scalar_string};
use crate::{
    capabilities::{Capability, VistaCapabilities, unsupported},
    column::Column,
    local,
    page::{Page, Preview},
    query::Condition,
    reference::{Reference, ReferenceKind},
    shell::TableShell,
    vista::Vista,
};

type Rec = Record<CborValue>;

impl ContainedShell {
    fn in_set(&self, record: &Rec) -> bool {
        self.conditions.iter().all(|c| local::matches(c, record))
    }

    async fn rows(&self) -> Result<IndexMap<String, Rec>> {
        let mut rows = self.im.list_values().await?;
        rows.retain(|_, r| self.in_set(r));
        Ok(rows)
    }

    /// Re-serialize the whole collection and persist it to the parent
    /// column. Runs after every mutation.
    async fn flush(&self) -> Result<()> {
        let rows = self.im.list_values().await?;
        let value = match self.kind {
            ReferenceKind::ContainsOne => rows
                .into_values()
                .next()
                .map(CborValue::from)
                .unwrap_or_else(|| CborValue::Map(Vec::new())),
            _ => CborValue::Array(rows.into_values().map(CborValue::from).collect()),
        };
        (self.writeback)(value).await
    }

    /// The id a record takes on an insert without one: its id-column value if
    /// present, the fixed id for `contains_one`, otherwise the next position.
    async fn next_id(&self, record: &Rec) -> Result<String> {
        if let Some(id) = self.schema.id_column.as_deref().and_then(|c| record.get(c)) {
            return Ok(cbor_scalar_string(id));
        }
        match self.kind {
            ReferenceKind::ContainsOne => Ok(ONE_ID.to_string()),
            _ => Ok(self.im.list_values().await?.len().to_string()),
        }
    }
}

#[async_trait]
impl TableShell for ContainedShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        &self.schema.columns
    }
    fn references(&self) -> &IndexMap<String, Reference> {
        &self.schema.references
    }
    fn id_column(&self) -> Option<&str> {
        self.schema.id_column.as_deref()
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
    fn driver_name(&self) -> &'static str {
        "contained"
    }
    fn preview(&self) -> Preview {
        Preview::new("contained").with_note(
            "embedded in the parent row: these records arrived with the parent and cost no \
             query of their own; preview the parent table to see what fetches them",
        )
    }

    async fn list(&self) -> Result<IndexMap<String, Rec>> {
        self.rows().await
    }
    async fn get(&self, id: &str) -> Result<Option<Rec>> {
        Ok(self.im.get_value(id).await?.filter(|r| self.in_set(r)))
    }
    async fn count(&self) -> Result<i64> {
        Ok(self.rows().await?.len() as i64)
    }
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        let rows = self.rows().await?;
        let total = rows.len() as i64;
        let window = rows.into_iter().skip(offset).take(limit).collect();
        Ok(Page::new(window).with_total(total))
    }

    /// Conditions evaluate against the record's own cells, so only columns
    /// the contained schema declares can narrow. Dotted paths are refused
    /// rather than matching nothing.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        if !self.schema.columns.contains_key(&condition.column) {
            return Err(error!(
                "Unknown column",
                column = condition.column.clone(),
                verb = "add_condition"
            ));
        }
        self.conditions.push(condition.clone());
        Ok(())
    }

    async fn insert(&self, id: Option<&str>, record: &Rec) -> Result<(String, Rec)> {
        let id = match id {
            Some(id) => id.to_string(),
            None => self.next_id(record).await?,
        };
        let stored = self.im.insert_value(id.clone(), record).await?;
        self.flush().await?;
        Ok((id, stored))
    }
    async fn replace(&self, id: &str, record: &Rec) -> Result<Rec> {
        let stored = self.im.replace_value(id, record).await?;
        self.flush().await?;
        Ok(stored)
    }
    /// The in-memory replace creates a missing row, so it is the upsert too.
    async fn upsert(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.replace(id, record).await
    }
    async fn patch(&self, id: &str, partial: &Rec) -> Result<Rec> {
        let stored = self.im.patch_value(id, partial).await?;
        self.flush().await?;
        Ok(stored)
    }
    async fn delete(&self, id: &str) -> Result<()> {
        self.im.delete(id).await?;
        self.flush().await
    }
    async fn delete_all(&self) -> Result<()> {
        self.im.delete_all().await?;
        self.flush().await
    }

    /// A relation of a contained record (a line's `product`), resolved back
    /// into the real table by the host's resolver.
    fn get_ref(&self, relation: &str, row: Option<&Rec>) -> Result<Vista> {
        let Some(resolve) = &self.ref_resolver else {
            return unsupported(Capability::Traverse);
        };
        let row = row.ok_or_else(|| {
            error!(
                "a contained record's relation needs the record",
                relation = relation
            )
            .mark_incorrect_usage()
        })?;
        resolve(relation, row)
    }
}
