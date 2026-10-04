//! [`ForwardShell`]: every [`TableShell`] method, defaulting to the inner
//! shell's answer.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

use crate::{
    AggregateSpec, Column, ContainedSpec, FilterOp, Reference, SortDirection, TableShell, Vista,
    VistaCapabilities, VistaChangeStream, VistaRowStream,
};

type Rec = Record<CborValue>;
type Rows = Vec<(String, Rec)>;

/// A [`TableShell`] that wraps another one. Every type implementing this is a
/// `TableShell`; each method defaults to the [`inner`](Self::inner) shell's
/// answer, so a wrapper overrides only what it changes — the schema
/// (`columns`), the capabilities, a read, a write.
///
/// [`clone_shell`](Self::clone_shell) has no default: a forwarded copy
/// would drop the wrapper. [`get_ref`](Self::get_ref) and
/// [`get_ref_target`](Self::get_ref_target) forward the inner target as is;
/// a wrapper whose behaviour should follow a reference re-wraps it.
///
/// Two derived methods go through the wrapper rather than the inner shell:
/// [`stream_vista_values`](Self::stream_vista_values) streams the wrapper's
/// own list, and [`upsert_vista_value`](Self::upsert_vista_value) uses its own
/// replace and insert. [`fetch_window_counted`](Self::fetch_window_counted)
/// and [`get_vista_value_with_row`](Self::get_vista_value_with_row) forward,
/// to keep the inner shell's stated total and row-aware fetch; override them
/// alongside `fetch_window` and `get_vista_value`.
///
/// Inside the impl both traits name each method, so a call to the wrapper's
/// own version is spelled `TableShell::fetch_window(self, ..)`.
///
/// [`forward_table_shell!`](crate::forward_table_shell) writes `inner`,
/// `inner_mut` and the `#[async_trait]` attribute for a wrapper holding its
/// inner shell in a field.
#[async_trait]
#[allow(clippy::ptr_arg)]
pub trait ForwardShell: Send + Sync + 'static {
    fn inner(&self) -> &dyn TableShell;
    fn inner_mut(&mut self) -> &mut dyn TableShell;
    fn clone_shell(&self) -> Option<Box<dyn TableShell>>;

    fn columns(&self) -> &IndexMap<String, Column> {
        self.inner().columns()
    }
    fn references(&self) -> &IndexMap<String, Reference> {
        self.inner().references()
    }
    fn contained(&self) -> &IndexMap<String, ContainedSpec> {
        self.inner().contained()
    }
    fn id_column(&self) -> Option<&str> {
        self.inner().id_column()
    }
    fn driver_name(&self) -> &'static str {
        self.inner().driver_name()
    }
    fn capabilities(&self) -> &VistaCapabilities {
        self.inner().capabilities()
    }
    fn preview_query(&self, vista: &Vista) -> serde_json::Value {
        self.inner().preview_query(vista)
    }
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, engine: &mut vantage_rhai::rhai::Engine) {
        self.inner().register_rhai_extensions(engine)
    }
    #[cfg(feature = "rhai")]
    fn rhai_env(&self, env: vantage_rhai::Env) -> vantage_rhai::Env {
        self.inner().rhai_env(env)
    }

    // ---- Reads -------------------------------------------------------------

    async fn list_vista_values(&self, vista: &Vista) -> Result<IndexMap<String, Rec>> {
        self.inner().list_vista_values(vista).await
    }
    async fn get_vista_value(&self, vista: &Vista, id: &String) -> Result<Option<Rec>> {
        self.inner().get_vista_value(vista, id).await
    }
    /// Forwards to the inner shell, so a driver that reads `row` (a cmd
    /// detail script) keeps it. A wrapper overriding
    /// [`get_vista_value`](Self::get_vista_value) overrides this too.
    async fn get_vista_value_with_row(
        &self,
        vista: &Vista,
        id: &String,
        row: &Rec,
    ) -> Result<Option<Rec>> {
        self.inner().get_vista_value_with_row(vista, id, row).await
    }
    async fn get_vista_some_value(&self, vista: &Vista) -> Result<Option<(String, Rec)>> {
        self.inner().get_vista_some_value(vista).await
    }
    /// Streams the wrapper's own [`list_vista_values`](Self::list_vista_values),
    /// so a wrapper that filters or rewrites rows there is not bypassed. A
    /// wrapper that changes no rows and wants the inner shell's native
    /// streaming overrides this to call `self.inner().stream_vista_values(..)`.
    fn stream_vista_values<'a>(&'a self, vista: &'a Vista) -> VistaRowStream<'a> {
        crate::stream_from_list(ForwardShell::list_vista_values(self, vista))
    }
    async fn get_vista_count(&self, vista: &Vista) -> Result<i64> {
        self.inner().get_vista_count(vista).await
    }
    fn aggregate_vista(&self, vista: &Vista, spec: &AggregateSpec) -> Result<Option<Vista>> {
        self.inner().aggregate_vista(vista, spec)
    }
    async fn fetch_page(&self, vista: &Vista, page: usize) -> Result<Rows> {
        self.inner().fetch_page(vista, page).await
    }
    async fn fetch_next(
        &self,
        vista: &Vista,
        token: Option<CborValue>,
    ) -> Result<(Rows, Option<CborValue>)> {
        self.inner().fetch_next(vista, token).await
    }
    async fn fetch_window(&self, vista: &Vista, offset: usize, limit: usize) -> Result<Rows> {
        self.inner().fetch_window(vista, offset, limit).await
    }
    /// Forwards to the inner shell, so the source's stated total survives. A
    /// wrapper overriding [`fetch_window`](Self::fetch_window) overrides this
    /// too, or the counted path serves the inner rows unchanged.
    async fn fetch_window_counted(
        &self,
        vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<(Rows, Option<i64>)> {
        self.inner()
            .fetch_window_counted(vista, offset, limit)
            .await
    }
    async fn watch_vista(&self, vista: &Vista) -> Result<VistaChangeStream> {
        self.inner().watch_vista(vista).await
    }

    // ---- Writes ------------------------------------------------------------

    async fn insert_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        self.inner().insert_vista_value(vista, id, record).await
    }
    async fn replace_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        self.inner().replace_vista_value(vista, id, record).await
    }
    /// Replaces through the wrapper's own
    /// [`replace_vista_value`](Self::replace_vista_value), falling back to its
    /// own [`insert_vista_value`](Self::insert_vista_value) on `NotFound`, so a
    /// wrapper guarding those two guards upsert as well.
    async fn upsert_vista_value(&self, vista: &Vista, id: &String, record: &Rec) -> Result<Rec> {
        match ForwardShell::replace_vista_value(self, vista, id, record).await {
            Err(e) if e.is_not_found() => {
                ForwardShell::insert_vista_value(self, vista, id, record).await
            }
            other => other,
        }
    }
    async fn patch_vista_value(&self, vista: &Vista, id: &String, partial: &Rec) -> Result<Rec> {
        self.inner().patch_vista_value(vista, id, partial).await
    }
    async fn delete_vista_value(&self, vista: &Vista, id: &String) -> Result<()> {
        self.inner().delete_vista_value(vista, id).await
    }
    async fn delete_vista_all_values(&self, vista: &Vista) -> Result<()> {
        self.inner().delete_vista_all_values(vista).await
    }
    async fn insert_vista_return_id_value(&self, vista: &Vista, record: &Rec) -> Result<String> {
        self.inner()
            .insert_vista_return_id_value(vista, record)
            .await
    }
    async fn import_vista_values(
        &self,
        vista: &Vista,
        records: &IndexMap<String, Rec>,
    ) -> Result<usize> {
        self.inner().import_vista_values(vista, records).await
    }

    // ---- Narrowing ---------------------------------------------------------

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        self.inner_mut().add_eq_condition(field, value)
    }
    fn add_op_condition(&mut self, field: &str, op: FilterOp, value: &CborValue) -> Result<()> {
        self.inner_mut().add_op_condition(field, op, value)
    }
    fn add_raw_condition(&mut self, condition: Box<dyn std::any::Any + Send + Sync>) -> Result<()> {
        self.inner_mut().add_raw_condition(condition)
    }
    fn set_page_size(&mut self, size: usize) -> Result<()> {
        self.inner_mut().set_page_size(size)
    }
    fn add_search(&mut self, text: &str) -> Result<()> {
        self.inner_mut().add_search(text)
    }
    fn clear_search(&mut self) -> Result<()> {
        self.inner_mut().clear_search()
    }
    fn add_order(&mut self, field: &str, dir: SortDirection) -> Result<()> {
        self.inner_mut().add_order(field, dir)
    }
    fn clear_orders(&mut self) -> Result<()> {
        self.inner_mut().clear_orders()
    }

    // ---- References --------------------------------------------------------

    fn get_ref(&self, relation: &str, row: &Rec) -> Result<Vista> {
        self.inner().get_ref(relation, row)
    }
    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        self.inner().get_ref_target(relation)
    }
    fn get_contained_ref(&self, relation: &str, row: &Rec) -> Result<Vista> {
        self.inner().get_contained_ref(relation, row)
    }
}
