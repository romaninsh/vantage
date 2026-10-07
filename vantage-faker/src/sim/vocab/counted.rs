//! A [`MemoryTableShell`] wrapper that bumps a shared counter on every write
//! that actually changes the store, so `SimEngine::stats().writes` counts the
//! writes scripts make through vantage-vista's `DataVocab` terminals.
//!
//! Everything else forwards through [`forward_table_shell!`], including the
//! write the counter does not see (`delete_vista_all_values`). Copies and
//! `ref(...)` targets stay wrapped, so their writes count as well.

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

    fn counted(&self, vista: Vista) -> Vista {
        let name = vista.name().to_string();
        let shell = CountedShell {
            inner: vista.source,
            table: None,
            writes: self.writes.clone(),
        };
        Vista::new(name, Box::new(shell))
    }
}

forward_table_shell!(CountedShell, inner, {
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        Some(Box::new(CountedShell {
            inner: self.inner.clone_shell()?,
            table: self.table.clone(),
            writes: self.writes.clone(),
        }))
    }

    fn get_ref(&self, relation: &str, row: &Rec) -> Result<Vista> {
        Ok(self.counted(self.inner.get_ref(relation, row)?))
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        Ok(self.counted(self.inner.get_ref_target(relation)?))
    }

    /// An insert of an existing id changes nothing and does not count.
    async fn insert_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        let before = self.table_writes();
        let row = self.inner.insert_vista_value(vista, id, record).await?;
        self.bump_if_changed(before);
        Ok(row)
    }

    async fn replace_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        let row = self.inner.replace_vista_value(vista, id, record).await?;
        self.bump();
        Ok(row)
    }

    /// An upsert that leaves the row as it was does not count.
    async fn upsert_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        let before = self.table_writes();
        let row = self.inner.upsert_vista_value(vista, id, record).await?;
        self.bump_if_changed(before);
        Ok(row)
    }

    async fn patch_vista_value(&self, vista: &Vista, id: &String, partial: &Rec) -> Result<Rec> {
        let row = self.inner.patch_vista_value(vista, id, partial).await?;
        self.bump();
        Ok(row)
    }

    async fn delete_vista_value(&self, vista: &Vista, id: &String) -> Result<()> {
        let before = self.table_writes();
        self.inner.delete_vista_value(vista, id).await?;
        self.bump_if_changed(before);
        Ok(())
    }

    async fn insert_vista_return_id_value(&self, vista: &Vista, record: &Rec) -> Result<String> {
        let id = self
            .inner
            .insert_vista_return_id_value(vista, record)
            .await?;
        self.bump();
        Ok(id)
    }

    async fn import_vista_values(
        &self,
        vista: &Vista,
        records: &IndexMap<String, Rec>,
    ) -> Result<usize> {
        let n = self.inner.import_vista_values(vista, records).await?;
        self.writes.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
});
