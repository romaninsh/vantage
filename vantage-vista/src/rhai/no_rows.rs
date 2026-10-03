//! The target of a multi-row `ref` step over no parent rows: the target
//! table's schema and writes, with a set that holds nothing.
//!
//! An empty key list can't be sent as an `in` condition (SQL rejects
//! `col IN ()`), so the empty set is answered here, the same on every backend.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

use crate::{
    Column, ContainedSpec, FilterOp, Reference, SortDirection, TableShell, Vista, VistaCapabilities,
};

type Rec = Record<CborValue>;

pub(crate) struct NoRowsShell {
    inner: Box<dyn TableShell>,
}

impl NoRowsShell {
    /// `target` as a set with no rows.
    pub(crate) fn wrap(target: Vista) -> Vista {
        let name = target.name().to_string();
        Vista::new(
            name,
            Box::new(NoRowsShell {
                inner: target.source,
            }),
        )
    }
}

#[async_trait]
impl TableShell for NoRowsShell {
    fn columns(&self) -> &IndexMap<String, Column> {
        self.inner.columns()
    }
    fn references(&self) -> &IndexMap<String, Reference> {
        self.inner.references()
    }
    fn contained(&self) -> &IndexMap<String, ContainedSpec> {
        self.inner.contained()
    }
    fn id_column(&self) -> Option<&str> {
        self.inner.id_column()
    }
    fn driver_name(&self) -> &'static str {
        self.inner.driver_name()
    }
    fn capabilities(&self) -> &VistaCapabilities {
        self.inner.capabilities()
    }
    fn preview_query(&self, _vista: &Vista) -> serde_json::Value {
        serde_json::json!({
            "driver": self.inner.driver_name(),
            "query": null,
            "note": "`ref` over no rows: nothing is queried",
        })
    }
    fn register_rhai_extensions(&self, engine: &mut vantage_rhai::rhai::Engine) {
        self.inner.register_rhai_extensions(engine)
    }
    fn rhai_env(&self, env: vantage_rhai::Env) -> vantage_rhai::Env {
        self.inner.rhai_env(env)
    }
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        let inner = self.inner.clone_shell()?;
        Some(Box::new(NoRowsShell { inner }))
    }

    async fn list_vista_values(&self, _vista: &Vista) -> Result<IndexMap<String, Rec>> {
        Ok(IndexMap::new())
    }
    async fn get_vista_value(&self, _vista: &Vista, _id: &String) -> Result<Option<Rec>> {
        Ok(None)
    }
    async fn get_vista_some_value(&self, _vista: &Vista) -> Result<Option<(String, Rec)>> {
        Ok(None)
    }
    async fn get_vista_count(&self, _vista: &Vista) -> Result<i64> {
        Ok(0)
    }
    async fn fetch_page(&self, _vista: &Vista, _page: usize) -> Result<Vec<(String, Rec)>> {
        Ok(Vec::new())
    }
    async fn fetch_window(
        &self,
        _vista: &Vista,
        _offset: usize,
        _limit: usize,
    ) -> Result<Vec<(String, Rec)>> {
        Ok(Vec::new())
    }
    async fn fetch_window_counted(
        &self,
        _vista: &Vista,
        _offset: usize,
        _limit: usize,
    ) -> Result<(Vec<(String, Rec)>, Option<i64>)> {
        Ok((Vec::new(), Some(0)))
    }

    // Narrowing an empty set leaves it empty; the target still checks the
    // step, so a bad column fails as it would on any set.
    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        self.inner.add_eq_condition(field, value)
    }
    fn add_op_condition(&mut self, field: &str, op: FilterOp, value: &CborValue) -> Result<()> {
        self.inner.add_op_condition(field, op, value)
    }
    fn set_page_size(&mut self, size: usize) -> Result<()> {
        self.inner.set_page_size(size)
    }
    fn add_search(&mut self, text: &str) -> Result<()> {
        self.inner.add_search(text)
    }
    fn clear_search(&mut self) -> Result<()> {
        self.inner.clear_search()
    }
    fn add_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        self.inner.add_order(field, dir)
    }
    fn clear_orders(&mut self) -> Result<()> {
        self.inner.clear_orders()
    }

    fn get_ref(&self, relation: &str, row: &Rec) -> Result<Vista> {
        self.inner.get_ref(relation, row)
    }
    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        self.inner.get_ref_target(relation)
    }

    // Writes by id go to the target table, as through any `ref` target.
    async fn insert_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        self.inner.insert_vista_value(vista, id, record).await
    }
    async fn insert_vista_return_id_value(&self, vista: &Vista, record: &Rec) -> Result<String> {
        self.inner.insert_vista_return_id_value(vista, record).await
    }
    async fn replace_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        self.inner.replace_vista_value(vista, id, record).await
    }
    async fn upsert_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        self.inner.upsert_vista_value(vista, id, record).await
    }
    async fn patch_vista_value(&self, vista: &Vista, id: &String, partial: &Rec) -> Result<Rec> {
        self.inner.patch_vista_value(vista, id, partial).await
    }
    async fn delete_vista_value(&self, vista: &Vista, id: &String) -> Result<()> {
        self.inner.delete_vista_value(vista, id).await
    }
    /// The set is empty, so there is nothing to delete.
    async fn delete_vista_all_values(&self, _vista: &Vista) -> Result<()> {
        Ok(())
    }
    async fn import_vista_values(
        &self,
        vista: &Vista,
        records: &IndexMap<String, Rec>,
    ) -> Result<usize> {
        self.inner.import_vista_values(vista, records).await
    }
}
