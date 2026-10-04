//! The [`TableShell`] impl of [`ShapedShell`]: every read, write and
//! query-state call is gated by the shape and tolled; the rest forwards.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

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
        self.gate(
            self.shape.capabilities.can_fetch_window,
            "fetch_window",
            "can_fetch_window",
        )?;
        tracing::debug!(target: "vantage_faker::shape", op = "window", offset, limit, "request");
        self.toll(OpClass::Window).await?;
        let offset = self.skewed_offset(offset);
        self.inner.fetch_window(vista, offset, limit).await
    }

    /// The total rides the same response envelope as the rows (no second
    /// toll) — and only exists where the shape can count.
    async fn fetch_window_counted(
        &self,
        vista: &Vista,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<i64>)> {
        let rows = TableShell::fetch_window(self, vista, offset, limit).await?;
        let total = if self.shape.capabilities.can_count {
            Some(self.lied_total(vista).await?)
        } else {
            None
        };
        Ok((rows, total))
    }

    async fn fetch_page(
        &self,
        vista: &Vista,
        page: usize,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        self.gate(
            self.shape.capabilities.can_fetch_page,
            "fetch_page",
            "can_fetch_page",
        )?;
        self.toll(OpClass::Window).await?;
        let offset = self.skewed_offset(page.saturating_sub(1) * self.page_size);
        self.inner.fetch_window(vista, offset, self.page_size).await
    }

    async fn fetch_next(
        &self,
        vista: &Vista,
        token: Option<CborValue>,
    ) -> Result<(Vec<(String, Record<CborValue>)>, Option<CborValue>)> {
        self.gate(
            self.shape.capabilities.can_fetch_next,
            "fetch_next",
            "can_fetch_next",
        )?;
        self.toll(OpClass::Window).await?;
        let offset = match &token {
            None => 0,
            Some(t) => self.decode_cursor(t)?,
        };
        let offset = self.skewed_offset(offset);
        let rows = self
            .inner
            .fetch_window(vista, offset, self.page_size)
            .await?;
        let next = (rows.len() == self.page_size).then(|| self.cursor_token(offset + rows.len()));
        Ok((rows, next))
    }

    async fn get_vista_count(&self, vista: &Vista) -> Result<i64> {
        self.gate(
            self.shape.capabilities.can_count,
            "get_vista_count",
            "can_count",
        )?;
        tracing::debug!(target: "vantage_faker::shape", op = "count", "request");
        self.toll(OpClass::Count).await?;
        self.lied_total(vista).await
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

    // ---- Writes: forwarded when advertised, no toll — the scenarios stress
    // the read path; write latency is a personality nobody asked for yet.

    async fn insert_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.gate(
            self.shape.capabilities.can_insert,
            "insert_vista_value",
            "can_insert",
        )?;
        self.inner.insert_vista_value(vista, id, record).await
    }

    async fn replace_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        self.gate(
            self.shape.capabilities.can_update,
            "replace_vista_value",
            "can_update",
        )?;
        self.inner.replace_vista_value(vista, id, record).await
    }

    /// A replace, then an insert when the row is missing, each gated.
    async fn upsert_vista_value(
        &self,
        vista: &Vista,
        id: &String,
        record: &Record<CborValue>,
    ) -> Result<Record<CborValue>> {
        match TableShell::replace_vista_value(self, vista, id, record).await {
            Err(e) if e.is_not_found() => {
                TableShell::insert_vista_value(self, vista, id, record).await
            }
            other => other,
        }
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
        self.gate(
            self.shape.capabilities.can_update,
            "patch_vista_value",
            "can_update",
        )?;
        self.inner.patch_vista_value(vista, id, partial).await
    }

    async fn delete_vista_value(&self, vista: &Vista, id: &String) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_delete,
            "delete_vista_value",
            "can_delete",
        )?;
        self.inner.delete_vista_value(vista, id).await
    }

    async fn delete_vista_all_values(&self, vista: &Vista) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_delete,
            "delete_vista_all_values",
            "can_delete",
        )?;
        self.inner.delete_vista_all_values(vista).await
    }

    async fn insert_vista_return_id_value(
        &self,
        vista: &Vista,
        record: &Record<CborValue>,
    ) -> Result<String> {
        self.gate(
            self.shape.capabilities.can_insert,
            "insert_vista_return_id_value",
            "can_insert",
        )?;
        self.inner.insert_vista_return_id_value(vista, record).await
    }

    // ---- Query state ------------------------------------------------------

    fn add_eq_condition(&mut self, field: &str, value: &CborValue) -> Result<()> {
        // Equality push-down is universal — not gated, per the capability
        // contract.
        self.inner.add_eq_condition(field, value)
    }

    fn add_op_condition(&mut self, field: &str, op: FilterOp, value: &CborValue) -> Result<()> {
        if op == FilterOp::Eq {
            return self.inner.add_eq_condition(field, value);
        }
        self.gate(
            self.shape.capabilities.can_filter_operators,
            "add_op_condition",
            "can_filter_operators",
        )?;
        self.inner.add_op_condition(field, op, value)
    }

    fn add_search(&mut self, text: &str) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_search,
            "add_search",
            "can_search",
        )?;
        self.inner.add_search(text)?;
        self.searching.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn clear_search(&mut self) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_search,
            "clear_search",
            "can_search",
        )?;
        self.inner.clear_search()?;
        self.searching.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn add_order(&mut self, field: &str, dir: vantage_vista::sort::SortDirection) -> Result<()> {
        self.gate(self.shape.capabilities.can_order, "add_order", "can_order")?;
        self.inner.add_order(field, dir)
    }

    fn clear_orders(&mut self) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_order,
            "clear_orders",
            "can_order",
        )?;
        self.inner.clear_orders()
    }

    fn set_page_size(&mut self, size: usize) -> Result<()> {
        self.gate(
            self.shape.capabilities.can_set_page_size,
            "set_page_size",
            "can_set_page_size",
        )?;
        self.page_size = size.max(1);
        Ok(())
    }

    /// Same store and fault/jitter stream, fresh query state — the clone a
    /// consumer narrows (`add_order` + `fetch_window`) without disturbing
    /// this handle. Each clone tracks its own `searching`.
    fn clone_shell(&self) -> Option<Box<dyn TableShell>> {
        let inner = self.inner.clone_shell()?;
        Some(Box::new(Self {
            inner,
            shape: self.shape.clone(),
            capabilities: self.capabilities.clone(),
            rng: self.rng.clone(),
            epoch: self.epoch,
            page_size: self.page_size,
            searching: Arc::new(AtomicBool::new(false)),
        }))
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
