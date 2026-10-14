//! `TableShell` for `MemoryTableShell`. Narrowing, writes, references and
//! live watching live in sibling modules.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{
    Condition, Page, Preview, SortDirection, TableShell, Vista, VistaCapabilities,
    VistaChangeStream, VistaMetadata,
};

use super::{MemoryTableShell, watch};
use crate::Row;
use crate::eval::matches_all;

type Rec = Record<CborValue>;

fn unwrap_rows(rows: Vec<(String, Row)>) -> Vec<(String, Rec)> {
    rows.into_iter()
        .map(|(id, row)| (id, (*row).clone()))
        .collect()
}

#[async_trait]
impl TableShell for MemoryTableShell {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }

    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "memory"
    }

    fn preview(&self) -> Preview {
        Preview::new().with_query(self.render_query())
    }

    async fn list(&self) -> Result<IndexMap<String, Rec>> {
        Ok(unwrap_rows(self.table.query(&self.query)?)
            .into_iter()
            .collect())
    }

    async fn get(&self, id: &str) -> Result<Option<Rec>> {
        match self.table.get(id) {
            Some(row) if matches_all(&self.query, &row)? => Ok(Some((*row).clone())),
            _ => Ok(None),
        }
    }

    async fn count(&self) -> Result<i64> {
        Ok(self.table.count(&self.query)? as i64)
    }

    /// The store counts the set as cheaply as it cuts the window, so the
    /// page always carries the total.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        let rows = unwrap_rows(self.table.query(&self.windowed(offset, Some(limit)))?);
        let total = self.table.count(&self.query)? as i64;
        Ok(Page::new(rows).with_total(total))
    }

    async fn watch(&self) -> Result<VistaChangeStream> {
        Ok(watch::stream(self.table().clone(), self.query().clone()))
    }

    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        self.narrow_by(condition)
    }

    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.replace_search(text);
        Ok(())
    }

    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        self.replace_order(order)
    }

    async fn insert(&self, id: Option<&str>, record: &Rec) -> Result<(String, Rec)> {
        match id {
            Some(id) => Ok((id.to_string(), self.insert_row(id, record)?)),
            None => self.insert_generated(record),
        }
    }

    async fn replace(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.replace_row(id, record)
    }

    async fn upsert(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.replace_row(id, record)
    }

    async fn patch(&self, id: &str, partial: &Rec) -> Result<Rec> {
        self.patch_row(id, partial)
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.delete_row(id)
    }

    async fn delete_all(&self) -> Result<()> {
        self.delete_matching()
    }

    async fn import(&self, rows: &IndexMap<String, Rec>) -> Result<usize> {
        self.import_rows(rows)
    }

    fn get_ref(&self, relation: &str, row: Option<&Rec>) -> Result<Vista> {
        self.traverse(relation, row)
    }
}
