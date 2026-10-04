//! Every [`ForwardShell`] is a [`TableShell`]: each method calls the
//! wrapper's `ForwardShell` version, whether overridden or forwarded.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

use super::ForwardShell;
use crate::{
    AggregateSpec, Column, ContainedSpec, FilterOp, Reference, SortDirection, TableShell, Vista,
    VistaCapabilities, VistaChangeStream, VistaRowStream,
};

type Rec = Record<CborValue>;
type Rows = Vec<(String, Rec)>;

#[async_trait]
#[allow(clippy::ptr_arg)]
impl<T: ForwardShell> TableShell for T {
    fn columns(&self) -> &IndexMap<String, Column> {
        ForwardShell::columns(self)
    }
    fn references(&self) -> &IndexMap<String, Reference> {
        ForwardShell::references(self)
    }
    fn contained(&self) -> &IndexMap<String, ContainedSpec> {
        ForwardShell::contained(self)
    }
    fn id_column(&self) -> Option<&str> {
        ForwardShell::id_column(self)
    }
    fn driver_name(&self) -> &'static str {
        ForwardShell::driver_name(self)
    }
    fn capabilities(&self) -> &VistaCapabilities {
        ForwardShell::capabilities(self)
    }
    fn preview_query(&self, vista: &Vista) -> serde_json::Value {
        ForwardShell::preview_query(self, vista)
    }
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, engine: &mut vantage_rhai::rhai::Engine) {
        ForwardShell::register_rhai_extensions(self, engine)
    }
    #[cfg(feature = "rhai")]
    fn rhai_env(&self, env: vantage_rhai::Env) -> vantage_rhai::Env {
        ForwardShell::rhai_env(self, env)
    }
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        ForwardShell::clone_shell(self)
    }

    async fn list_vista_values(&self, vista: &Vista) -> Result<IndexMap<String, Rec>> {
        ForwardShell::list_vista_values(self, vista).await
    }
    async fn get_vista_value(&self, vista: &Vista, id: &String) -> Result<Option<Rec>> {
        ForwardShell::get_vista_value(self, vista, id).await
    }
    async fn get_vista_value_with_row(
        &self,
        vista: &Vista,
        id: &String,
        row: &Rec,
    ) -> Result<Option<Rec>> {
        ForwardShell::get_vista_value_with_row(self, vista, id, row).await
    }
    async fn get_vista_some_value(&self, vista: &Vista) -> Result<Option<(String, Rec)>> {
        ForwardShell::get_vista_some_value(self, vista).await
    }
    fn stream_vista_values<'a>(&'a self, vista: &'a Vista) -> VistaRowStream<'a>
    where
        Self: Sync,
    {
        ForwardShell::stream_vista_values(self, vista)
    }
    async fn get_vista_count(&self, vista: &Vista) -> Result<i64> {
        ForwardShell::get_vista_count(self, vista).await
    }
    fn aggregate_vista(&self, vista: &Vista, spec: &AggregateSpec) -> Result<Option<Vista>> {
        ForwardShell::aggregate_vista(self, vista, spec)
    }
    async fn fetch_page(&self, vista: &Vista, page: usize) -> Result<Rows> {
        ForwardShell::fetch_page(self, vista, page).await
    }
    async fn fetch_next(
        &self,
        vista: &Vista,
        token: Option<CborValue>,
    ) -> Result<(Rows, Option<CborValue>)> {
        ForwardShell::fetch_next(self, vista, token).await
    }
    async fn fetch_window(&self, vista: &Vista, offset: usize, limit: usize) -> Result<Rows> {
        ForwardShell::fetch_window(self, vista, offset, limit).await
    }
    async fn fetch_window_counted(
        &self,
        vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<(Rows, Option<i64>)> {
        ForwardShell::fetch_window_counted(self, vista, offset, limit).await
    }
    async fn watch_vista(&self, vista: &Vista) -> Result<VistaChangeStream> {
        ForwardShell::watch_vista(self, vista).await
    }

    async fn insert_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        ForwardShell::insert_vista_value(self, vista, id, record).await
    }
    async fn replace_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        ForwardShell::replace_vista_value(self, vista, id, record).await
    }
    async fn upsert_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        ForwardShell::upsert_vista_value(self, vista, id, record).await
    }
    async fn patch_vista_value(&self, vista: &Vista, id: &String, partial: &Rec) -> Result<Rec> {
        ForwardShell::patch_vista_value(self, vista, id, partial).await
    }
    async fn delete_vista_value(&self, vista: &Vista, id: &String) -> Result<()> {
        ForwardShell::delete_vista_value(self, vista, id).await
    }
    async fn delete_vista_all_values(&self, vista: &Vista) -> Result<()> {
        ForwardShell::delete_vista_all_values(self, vista).await
    }
    async fn insert_vista_return_id_value(&self, vista: &Vista, record: &Rec) -> Result<String> {
        ForwardShell::insert_vista_return_id_value(self, vista, record).await
    }
    async fn import_vista_values(
        &self,
        vista: &Vista,
        records: &IndexMap<String, Rec>,
    ) -> Result<usize> {
        ForwardShell::import_vista_values(self, vista, records).await
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        ForwardShell::add_eq_condition(self, field, value)
    }
    fn add_op_condition(&mut self, field: &str, op: FilterOp, value: &CborValue) -> Result<()> {
        ForwardShell::add_op_condition(self, field, op, value)
    }
    fn add_raw_condition(&mut self, condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()> {
        ForwardShell::add_raw_condition(self, condition)
    }
    fn set_page_size(&mut self, size: usize) -> Result<()> {
        ForwardShell::set_page_size(self, size)
    }
    fn add_search(&mut self, text: &str) -> Result<()> {
        ForwardShell::add_search(self, text)
    }
    fn clear_search(&mut self) -> Result<()> {
        ForwardShell::clear_search(self)
    }
    fn add_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        ForwardShell::add_order(self, field, dir)
    }
    fn clear_orders(&mut self) -> Result<()> {
        ForwardShell::clear_orders(self)
    }

    fn get_ref(&self, relation: &str, row: &Rec) -> Result<Vista> {
        ForwardShell::get_ref(self, relation, row)
    }
    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        ForwardShell::get_ref_target(self, relation)
    }
    fn get_contained_ref(&self, relation: &str, row: &Rec) -> Result<Vista> {
        ForwardShell::get_contained_ref(self, relation, row)
    }
}
