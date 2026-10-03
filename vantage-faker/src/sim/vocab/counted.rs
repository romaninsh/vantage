//! A [`MemoryTableShell`] wrapper that bumps a shared counter on every write
//! that actually changes the store, so `SimEngine::stats().writes` counts the
//! writes scripts make through vantage-vista's `DataVocab` terminals.
//!
//! Reads forward through [`forward_table_shell!`]; writes the counter does
//! not see (`delete_vista_all_values`) forward too.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_memory::MemoryTableShell;
use vantage_types::Record;
use vantage_vista::{TableShell, Vista, VistaCapabilities, forward_table_shell};

use crate::sim::stats::Counters;

type Rec = Record<CborValue>;

pub(super) struct CountedShell {
    inner: MemoryTableShell,
    writes: Arc<AtomicU64>,
}

impl CountedShell {
    pub(super) fn new(inner: MemoryTableShell, writes: Arc<AtomicU64>) -> Self {
        Self { inner, writes }
    }

    fn bump(&self) {
        Counters::bump(&self.writes);
    }
}

forward_table_shell!(CountedShell, inner, {
    fn capabilities(&self) -> &VistaCapabilities {
        self.inner.capabilities()
    }

    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        self.inner.clone_shell()
    }

    fn get_ref(&self, relation: &str, row: &Rec) -> Result<Vista> {
        self.inner.get_ref(relation, row)
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        self.inner.get_ref_target(relation)
    }

    async fn insert_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        let row = self.inner.insert_vista_value(vista, id, record).await?;
        self.bump();
        Ok(row)
    }

    async fn replace_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        let row = self.inner.replace_vista_value(vista, id, record).await?;
        self.bump();
        Ok(row)
    }

    /// An upsert that leaves the row as it was does not count: the store
    /// table's own write counter moves only when a row changes.
    async fn upsert_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        let table = self.inner.table();
        let before = table.writes();
        let row = self.inner.upsert_vista_value(vista, id, record).await?;
        if table.writes() != before {
            self.bump();
        }
        Ok(row)
    }

    async fn patch_vista_value(&self, vista: &Vista, id: &String, partial: &Rec) -> Result<Rec> {
        let row = self.inner.patch_vista_value(vista, id, partial).await?;
        self.bump();
        Ok(row)
    }

    async fn delete_vista_value(&self, vista: &Vista, id: &String) -> Result<()> {
        self.inner.delete_vista_value(vista, id).await?;
        self.bump();
        Ok(())
    }

    async fn delete_vista_all_values(&self, vista: &Vista) -> Result<()> {
        self.inner.delete_vista_all_values(vista).await
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
        if n > 0 {
            self.bump();
        }
        Ok(n)
    }
});
