//! The [`TableShell`] impl of [`ShapedShell`]: every read, write and
//! query-state call is gated by the shape and tolled; the rest forwards.
//!
//! The shaped bodies live in [`reads`], [`writes`] and [`query_state`]; the
//! impl below is one block because the forwarding macro emits a single trait
//! impl.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::capabilities::VistaCapabilities;
use vantage_vista::source::{TableShell, VistaChangeStream};
use vantage_vista::{
    AggregateSpec, FilterOp, Vista, VistaRowStream, forward_table_shell, stream_from_list,
};

use super::{OpClass, ShapedShell};

mod query_state;
mod reads;
mod writes;

// The schema, contained relations, raw conditions and Rhai hooks forward to
// the wrapped shell; every read, write and query-state call is shaped below.
forward_table_shell!(ShapedShell, inner, {
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "faker-shaped"
    }

    /// Shaping adds latency, page-size negotiation and injected faults around
    /// another shell; it does not change the query. So the wrapped shell's
    /// preview is the answer, tagged with the shaping around it.
    fn preview_query(&self, vista: &Vista) -> serde_json::Value {
        serde_json::json!({
            "driver": "faker-shaped",
            "page_size": self.page_size,
            "note": "simulated backend: latency and faults are layered over the \
                     wrapped shell without altering its query",
            "shaped": self.inner.preview_query(vista),
        })
    }

    async fn list_vista_values(
        &self,
        vista: &Vista,
    ) -> Result<IndexMap<String, Record<CborValue>>> {
        tracing::debug!(target: "vantage_faker::shape", op = "list", "request");
        self.toll(OpClass::List).await?;
        self.inner.list_vista_values(vista).await
    }

    async fn get_vista_value(
        &self,
        vista: &Vista,
        id: &String,
    ) -> Result<Option<Record<CborValue>>> {
        tracing::debug!(target: "vantage_faker::shape", op = "get", id = %id, "request");
        self.toll(OpClass::Get).await?;
        self.inner.get_vista_value(vista, id).await
    }

    async fn get_vista_some_value(
        &self,
        vista: &Vista,
    ) -> Result<Option<(String, Record<CborValue>)>> {
        self.toll(OpClass::Get).await?;
        self.inner.get_vista_some_value(vista).await
    }

    async fn get_vista_value_with_row(
        &self,
        vista: &Vista,
        id: &String,
        _row: &Record<CborValue>,
    ) -> Result<Option<Record<CborValue>>> {
        TableShell::get_vista_value(self, vista, id).await
    }

    /// Streams the tolled [`list_vista_values`](TableShell::list_vista_values).
    fn stream_vista_values<'a>(&'a self, vista: &'a Vista) -> VistaRowStream<'a> {
        stream_from_list(TableShell::list_vista_values(self, vista))
    }

    /// A shaped backend aggregates nothing itself; callers reduce locally.
    fn aggregate_vista(&self, _vista: &Vista, _spec: &AggregateSpec) -> Result<Option<Vista>> {
        Ok(None)
    }

    async fn fetch_window(
        &self,
        vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        self.shaped_fetch_window(vista, offset, limit).await
    }

    async fn fetch_window_counted(
        &self,
        vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<i64>)> {
        self.shaped_fetch_window_counted(vista, offset, limit).await
    }

    async fn fetch_page(
        &self,
        vista: &Vista,
        page: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        self.shaped_fetch_page(vista, page).await
    }

    async fn fetch_next(
        &self,
        vista: &Vista,
        token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        self.shaped_fetch_next(vista, token).await
    }

    async fn get_vista_count(&self, vista: &Vista) -> Result<i64> {
        self.shaped_count(vista).await
    }

    /// Shaping does not touch push delivery — no toll, no fault draw — so a
    /// subscribed Dio sees the wrapped shell's changes exactly as it emits
    /// them.
    async fn watch_vista(&self, vista: &Vista) -> Result<VistaChangeStream> {
        self.gate(
            self.capabilities.can_subscribe,
            "watch_vista",
            "can_subscribe",
        )?;
        self.inner.watch_vista(vista).await
    }

    async fn insert_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.shaped_insert(vista, id, record).await
    }

    async fn replace_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.shaped_replace(vista, id, record).await
    }

    async fn upsert_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.shaped_upsert(vista, id, record).await
    }

    /// Bulk import is not part of any shape.
    async fn import_vista_values(
        &self,
        _vista: &Vista,
        _records: &IndexMap<String, Record<CborValue>>,
    ) -> Result<usize> {
        Err(self.default_error("import_vista_values", "can_import"))
    }

    async fn patch_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        partial: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.shaped_patch(vista, id, partial).await
    }

    async fn delete_vista_value(&self, vista: &Vista, id: &String) -> Result<()> {
        self.shaped_delete(vista, id).await
    }

    async fn delete_vista_all_values(&self, vista: &Vista) -> Result<()> {
        self.shaped_delete_all(vista).await
    }

    async fn insert_vista_return_id_value(
        &self,
        vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        self.shaped_insert_return_id(vista, record).await
    }

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        // Equality push-down is universal — not gated, per the capability
        // contract.
        self.inner.add_eq_condition(field, value)
    }

    fn add_op_condition(&mut self, field: &str, op: FilterOp, value: &CborValue) -> Result<()> {
        self.shaped_add_op_condition(field, op, value)
    }

    fn add_search(&mut self, text: &str) -> Result<()> {
        self.shaped_add_search(text)
    }

    fn clear_search(&mut self) -> Result<()> {
        self.shaped_clear_search()
    }

    fn add_order(&mut self, field: &str, dir: vantage_vista::sort::SortDirection) -> Result<()> {
        self.shaped_add_order(field, dir)
    }

    fn clear_orders(&mut self) -> Result<()> {
        self.shaped_clear_orders()
    }

    fn set_page_size(&mut self, size: usize) -> Result<()> {
        self.shaped_set_page_size(size)
    }

    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        self.shaped_clone()
    }

    /// A target reached through a reference would not be shaped, so
    /// references are not followed.
    fn get_ref(&self, relation: &str, _row: &Record<CborValue>) -> Result<Vista> {
        Err(error!(
            "Shaped source doesn't follow references",
            relation = relation
        )
        .mark_unimplemented())
    }

    fn get_ref_target(&self, relation: &str) -> Result<Vista> {
        Err(error!(
            "Shaped source doesn't follow references",
            relation = relation
        )
        .mark_unimplemented())
    }
});
