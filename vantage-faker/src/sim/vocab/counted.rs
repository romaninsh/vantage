//! A [`MemoryTableShell`] wrapper that bumps a shared counter on every write
//! that actually changes the store, so `SimEngine::stats().writes` counts the
//! writes scripts make through vantage-vista's `DataVocab` terminals.
//!
//! Everything else forwards through [`forward_table_shell!`], including the
//! write the counter does not see (`delete_all`). Clones and `ref(...)`
//! targets stay wrapped, so their writes count as well.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_memory::{MemoryTableHandle, MemoryTableShell};
use vantage_types::Record;
use vantage_vista::{TableShell, Vista, forward_table_shell};

use crate::sim::stats::Counters;

type Rec = Record<CborValue>;

#[derive(Clone)]
pub(super) struct CountedShell {
    inner: Box<dyn TableShell>,
    /// The store table, when known, so an upsert that changes nothing isn't
    /// counted. A `ref(...)` target arrives without it.
    table: Option<MemoryTableHandle>,
    writes: Arc<AtomicU64>,
}

impl CountedShell {
    pub(super) fn new(inner: MemoryTableShell, writes: Arc<AtomicU64>) -> Self {
        Self {
            table: Some(inner.table().clone()),
            inner: Box::new(inner),
            writes,
        }
    }

    fn bump(&self) {
        Counters::bump(&self.writes);
    }

    /// The store table's write count, when the table is known.
    fn table_writes(&self) -> Option<u64> {
        self.table.as_ref().map(|t| t.writes())
    }

    /// Count a write unless the table is known and its own counter did not
    /// move since `before`.
    fn bump_if_changed(&self, before: Option<u64>) {
        if before.is_none() || before != self.table_writes() {
            self.bump();
        }
    }

    /// `vista` with its shell wrapped to count into the same counter; its
    /// name and applied query are kept.
    fn counted(&self, vista: Vista) -> Vista {
        let writes = self.writes.clone();
        vista.map_shell(|inner| CountedShell {
            inner,
            table: None,
            writes,
        })
    }
}

forward_table_shell!(CountedShell, inner, {
    fn get_ref(&self, relation: &str, row: Option<&Rec>) -> Result<Vista> {
        Ok(self.counted(self.inner.get_ref(relation, row)?))
    }

    /// An insert of an existing id changes nothing and does not count.
    async fn insert(&self, id: Option<&str>, record: &Rec) -> Result<(String, Rec)> {
        let before = self.table_writes();
        let stored = self.inner.insert(id, record).await?;
        self.bump_if_changed(before);
        Ok(stored)
    }

    async fn replace(&self, id: &str, record: &Rec) -> Result<Rec> {
        let row = self.inner.replace(id, record).await?;
        self.bump();
        Ok(row)
    }

    /// An upsert that leaves the row as it was does not count.
    async fn upsert(&self, id: &str, record: &Rec) -> Result<Rec> {
        let before = self.table_writes();
        let row = self.inner.upsert(id, record).await?;
        self.bump_if_changed(before);
        Ok(row)
    }

    async fn patch(&self, id: &str, partial: &Rec) -> Result<Rec> {
        let row = self.inner.patch(id, partial).await?;
        self.bump();
        Ok(row)
    }

    async fn delete(&self, id: &str) -> Result<()> {
        let before = self.table_writes();
        self.inner.delete(id).await?;
        self.bump_if_changed(before);
        Ok(())
    }

    async fn import(&self, rows: &IndexMap<String, Rec>) -> Result<usize> {
        let n = self.inner.import(rows).await?;
        self.writes.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
});

#[cfg(test)]
mod tests;
