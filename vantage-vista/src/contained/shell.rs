//! The [`TableShell`] behaviour of [`ContainedShell`].

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_types::Record;

use super::{ContainedShell, ContainedWriteback, ONE_ID, cbor_scalar_string};
use crate::{
    capabilities::{Capability, VistaCapabilities, unknown_relation, unsupported},
    metadata::VistaMetadata,
    page::{Page, Preview},
    query::Condition,
    reference::ReferenceKind,
    refuse::condition_column_error,
    shell::TableShell,
    vista::Vista,
};

type Rec = Record<CborValue>;

impl ContainedShell {
    fn in_set(&self, record: &Rec) -> bool {
        self.conditions
            .iter()
            .all(|c| c.matches(record, &self.schema))
    }

    async fn rows(&self) -> Result<IndexMap<String, Rec>> {
        let mut rows = self.im.list_values().await?;
        rows.retain(|_, r| self.in_set(r));
        Ok(rows)
    }

    /// The writeback a write flushes through, checked before the write
    /// touches the rows. A bare target has no parent row, so `capability`
    /// is refused.
    fn writeback(&self, capability: Capability) -> Result<&ContainedWriteback> {
        match &self.writeback {
            Some(writeback) => Ok(writeback),
            None => unsupported(capability),
        }
    }

    /// Re-serialize the whole collection and persist it to the parent
    /// column through `writeback`. Runs after every mutation.
    async fn flush(&self, writeback: &ContainedWriteback) -> Result<()> {
        let rows = self.im.list_values().await?;
        let value = match self.kind {
            ReferenceKind::ContainsOne => rows
                .into_values()
                .next()
                .map(CborValue::from)
                .unwrap_or_else(|| CborValue::Map(Vec::new())),
            _ => CborValue::Array(rows.into_values().map(CborValue::from).collect()),
        };
        writeback(value).await
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
    fn metadata(&self) -> &VistaMetadata {
        &self.schema
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
    fn driver_name(&self) -> &'static str {
        "contained"
    }
    fn preview(&self) -> Preview {
        Preview::new().with_note(
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
        Ok(Page::from_listing(self.rows().await?, offset, limit))
    }

    /// Conditions evaluate against the record's own cells, so only columns
    /// the contained schema declares can narrow. A dotted path into a
    /// declared column is refused with [`condition_column_error`], for the
    /// caller to evaluate; any other undeclared column is an error.
    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        if !self.schema.columns.contains_key(&condition.column) {
            return Err(condition_column_error(&self.schema, &condition.column));
        }
        self.conditions.push(condition.clone());
        Ok(())
    }

    async fn insert(&self, id: Option<&str>, record: &Rec) -> Result<(String, Rec)> {
        let writeback = self.writeback(Capability::Insert)?;
        let id = match id {
            Some(id) => id.to_string(),
            None => self.next_id(record).await?,
        };
        let stored = self.im.insert_value(id.clone(), record).await?;
        self.flush(writeback).await?;
        Ok((id, stored))
    }
    async fn replace(&self, id: &str, record: &Rec) -> Result<Rec> {
        let writeback = self.writeback(Capability::Update)?;
        let stored = self.im.replace_value(id, record).await?;
        self.flush(writeback).await?;
        Ok(stored)
    }
    /// The in-memory replace creates a missing row, so it is the upsert too.
    async fn upsert(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.replace(id, record).await
    }
    async fn patch(&self, id: &str, partial: &Rec) -> Result<Rec> {
        let writeback = self.writeback(Capability::Update)?;
        let stored = self.im.patch_value(id, partial).await?;
        self.flush(writeback).await?;
        Ok(stored)
    }
    async fn delete(&self, id: &str) -> Result<()> {
        let writeback = self.writeback(Capability::Delete)?;
        self.im.delete(id).await?;
        self.flush(writeback).await
    }
    async fn delete_all(&self) -> Result<()> {
        let writeback = self.writeback(Capability::Delete)?;
        self.im.delete_all().await?;
        self.flush(writeback).await
    }

    /// A relation of a contained record (a line's `product`): the bare
    /// target the host's resolver gives for the relation's target name,
    /// narrowed to `row` when one is given.
    fn get_ref(&self, relation: &str, row: Option<&Rec>) -> Result<Vista> {
        let Some(resolver) = &self.resolver else {
            return unsupported(Capability::Traverse);
        };
        let reference = self
            .schema
            .references
            .get(relation)
            .ok_or_else(|| unknown_relation(relation))?;
        let target = resolver.vista(&reference.target)?;
        match row {
            Some(row) => reference.narrow(target, row),
            None => Ok(target),
        }
    }
}
