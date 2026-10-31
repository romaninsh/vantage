//! The [`TableShell`] impl of [`ShapedShell`]: every read, write and
//! narrowing call is gated by the shape and tolled; the rest forwards.
//!
//! The paged reads live in [`reads`] and the narrowing setters in
//! [`query_state`]; the impl below is one block because the forwarding macro
//! emits a single trait impl. Writes are gated but not tolled: the scenarios
//! stress the read path.

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;
use vantage_vista::{
    Capability, Condition, NativeCondition, Page, SortDirection, TableShell, Transaction, Vista,
    VistaCapabilities, VistaChangeStream, forward_table_shell, unsupported,
};

use super::{OpClass, ShapedShell};

mod query_state;
mod reads;

type Rec = Record<CborValue>;

// The schema, the Rhai hooks and the preview forward to the wrapped shell:
// shaping adds latency and faults but never changes the query, and a
// narrowing the shape refuses errors when set, so it never reaches the
// preview. Every read, write and narrowing call is shaped below.
forward_table_shell!(ShapedShell, inner, {
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }

    fn driver_name(&self) -> &'static str {
        "faker-shaped"
    }

    async fn list(&self) -> Result<IndexMap<String, Rec>> {
        tracing::debug!(target: "vantage_faker::shape", op = "list", "request");
        self.toll(OpClass::List).await?;
        self.inner.list().await
    }

    async fn get(&self, id: &str) -> Result<Option<Rec>> {
        tracing::debug!(target: "vantage_faker::shape", op = "get", id = %id, "request");
        self.toll(OpClass::Get).await?;
        self.inner.get(id).await
    }

    async fn count(&self) -> Result<i64> {
        self.shaped_count().await
    }

    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        self.shaped_fetch_window(offset, limit).await
    }

    async fn fetch_cursor(
        &self,
        token: Option<CborValue>,
        limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        self.shaped_fetch_cursor(token, limit).await
    }

    /// Shaping does not touch push delivery — no toll, no fault draw — so a
    /// subscribed consumer sees the wrapped shell's changes exactly as it
    /// emits them.
    async fn watch(&self) -> Result<VistaChangeStream> {
        self.gate(Capability::Subscribe)?;
        self.inner.watch().await
    }

    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        self.shaped_add_condition(condition)
    }

    fn add_native_condition(&mut self, condition: NativeCondition) -> Result<()> {
        self.gate(Capability::NativeConditions)?;
        self.inner.add_native_condition(condition)
    }

    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.shaped_set_search(text)
    }

    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        self.gate(Capability::Order)?;
        self.inner.set_order(order)
    }

    async fn insert(&self, id: Option<&str>, record: &Rec) -> Result<(String, Rec)> {
        self.gate(Capability::Insert)?;
        self.inner.insert(id, record).await
    }

    async fn replace(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.gate(Capability::Update)?;
        self.inner.replace(id, record).await
    }

    async fn upsert(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.gate(Capability::Update)?;
        self.inner.upsert(id, record).await
    }

    async fn patch(&self, id: &str, partial: &Rec) -> Result<Rec> {
        self.gate(Capability::Update)?;
        self.inner.patch(id, partial).await
    }

    async fn delete(&self, id: &str) -> Result<()> {
        self.gate(Capability::Delete)?;
        self.inner.delete(id).await
    }

    async fn delete_all(&self) -> Result<()> {
        self.gate(Capability::Delete)?;
        self.inner.delete_all().await
    }

    /// Bulk import is not part of any shape.
    async fn import(&self, _rows: &IndexMap<String, Rec>) -> Result<usize> {
        unsupported(Capability::Import)
    }

    /// A target reached through a reference would not be shaped, so
    /// references are not followed.
    fn get_ref(&self, _relation: &str, _row: Option<&Rec>) -> Result<Vista> {
        unsupported(Capability::Traverse)
    }

    /// The wrapped shell takes part, transactional or direct; the shape
    /// keeps shaping it.
    fn rebind(&self, tx: &Transaction) -> Result<Box<dyn TableShell>> {
        Ok(Box::new(Self {
            inner: self.inner.rebind(tx)?,
            ..self.clone()
        }))
    }
});
