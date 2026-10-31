//! `TableShell` for `MemoryTableShell`. Narrowing, writes, references,
//! live watching and transactions live in sibling modules.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{
    Condition, Page, Preview, SortDirection, TableShell, Transaction, Vista, VistaCapabilities,
    VistaChangeStream, VistaMetadata,
};

use super::{MemoryTableShell, MemoryTx, watch};
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
        self.on_rows(|s| Ok(unwrap_rows(s.table.query(&s.query)?).into_iter().collect()))
            .await
    }

    async fn get(&self, id: &str) -> Result<Option<Rec>> {
        self.on_rows(|s| match s.table.get(id) {
            Some(row) if matches_all(&s.query, &row)? => Ok(Some((*row).clone())),
            _ => Ok(None),
        })
        .await
    }

    async fn count(&self) -> Result<i64> {
        self.on_rows(|s| Ok(s.table.count(&s.query)? as i64)).await
    }

    /// The store counts the set as cheaply as it cuts the window, so the
    /// page always carries the total.
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        self.on_rows(|s| {
            let rows = unwrap_rows(s.table.query(&s.windowed(offset, Some(limit)))?);
            let total = s.table.count(&s.query)? as i64;
            Ok(Page::new(rows).with_total(total))
        })
        .await
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
        self.on_rows(|s| match id {
            Some(id) => Ok((id.to_string(), s.insert_row(id, record)?)),
            None => s.insert_generated(record),
        })
        .await
    }

    async fn replace(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.on_rows(|s| s.replace_row(id, record)).await
    }

    async fn upsert(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.on_rows(|s| s.replace_row(id, record)).await
    }

    async fn patch(&self, id: &str, partial: &Rec) -> Result<Rec> {
        self.on_rows(|s| s.patch_row(id, partial)).await
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.on_rows(|s| s.delete_row(id)).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.on_rows(|s| s.delete_matching()).await
    }

    async fn import(&self, rows: &IndexMap<String, Rec>) -> Result<usize> {
        self.on_rows(|s| s.import_rows(rows)).await
    }

    fn get_ref(&self, relation: &str, row: Option<&Rec>) -> Result<Vista> {
        self.traverse(relation, row)
    }

    /// Joins the store's transaction; nothing is claimed or copied until
    /// the first read or write (see the `tx` module).
    fn rebind(&self, tx: &Transaction) -> Result<Box<dyn TableShell>> {
        let store = self.catalog.store();
        let joined = tx.participant(MemoryTx::key(store), || MemoryTx::new(store, tx))?;
        Ok(Box::new(Self {
            tx: Some(joined),
            ..self.clone()
        }))
    }
}
