//! [`ForwardShell`]: every [`TableShell`] method, defaulting to the inner
//! shell's answer.

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::Result;
use vantage_types::Record;

use crate::{
    Condition, NativeCondition, Page, Preview, SortDirection, TableShell, Transaction, Vista,
    VistaCapabilities, VistaChangeStream, VistaMetadata, transaction::refuse_rebind,
};

type Rec = Record<CborValue>;

/// The trait [`forward_table_shell!`](crate::forward_table_shell) implements.
/// Every `ForwardShell + Clone` is a [`TableShell`]; each method defaults to
/// the [`inner`](Self::inner) shell's answer, so a wrapper overrides only
/// what it changes. The exception is [`rebind`](Self::rebind): it refuses
/// unless the wrapper overrides it, because only the wrapper knows how to
/// rebuild itself over a rebound inner shell.
///
/// [`get_ref`](Self::get_ref) forwards the inner target as is; a wrapper
/// whose behaviour should follow a reference re-wraps it.
#[async_trait]
pub trait ForwardShell: Send + Sync + 'static {
    fn inner(&self) -> &dyn TableShell;
    fn inner_mut(&mut self) -> &mut dyn TableShell;

    fn metadata(&self) -> &VistaMetadata {
        self.inner().metadata()
    }
    fn capabilities(&self) -> &VistaCapabilities {
        self.inner().capabilities()
    }
    fn driver_name(&self) -> &'static str {
        self.inner().driver_name()
    }
    fn preview(&self) -> Preview {
        self.inner().preview()
    }
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, engine: &mut vantage_rhai::rhai::Engine) {
        self.inner().register_rhai_extensions(engine)
    }

    // ---- Reads -------------------------------------------------------------

    async fn list(&self) -> Result<IndexMap<String, Rec>> {
        self.inner().list().await
    }
    async fn get(&self, id: &str) -> Result<Option<Rec>> {
        self.inner().get(id).await
    }
    async fn count(&self) -> Result<i64> {
        self.inner().count().await
    }
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        self.inner().fetch_window(offset, limit).await
    }
    async fn fetch_cursor(
        &self,
        token: Option<CborValue>,
        limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        self.inner().fetch_cursor(token, limit).await
    }
    async fn watch(&self) -> Result<VistaChangeStream> {
        self.inner().watch().await
    }

    // ---- Narrowing ---------------------------------------------------------

    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        self.inner_mut().add_condition(condition)
    }
    fn add_native_condition(&mut self, condition: NativeCondition) -> Result<()> {
        self.inner_mut().add_native_condition(condition)
    }
    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.inner_mut().set_search(text)
    }
    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        self.inner_mut().set_order(order)
    }

    // ---- Writes ------------------------------------------------------------

    async fn insert(&self, id: Option<&str>, record: &Rec) -> Result<(String, Rec)> {
        self.inner().insert(id, record).await
    }
    async fn replace(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.inner().replace(id, record).await
    }
    async fn upsert(&self, id: &str, record: &Rec) -> Result<Rec> {
        self.inner().upsert(id, record).await
    }
    async fn patch(&self, id: &str, partial: &Rec) -> Result<Rec> {
        self.inner().patch(id, partial).await
    }
    async fn delete(&self, id: &str) -> Result<()> {
        self.inner().delete(id).await
    }
    async fn delete_all(&self) -> Result<()> {
        self.inner().delete_all().await
    }
    async fn import(&self, rows: &IndexMap<String, Rec>) -> Result<usize> {
        self.inner().import(rows).await
    }

    // ---- References --------------------------------------------------------

    fn get_ref(&self, relation: &str, row: Option<&Rec>) -> Result<Vista> {
        self.inner().get_ref(relation, row)
    }

    // ---- Transactions ------------------------------------------------------

    /// Refuses unless overridden: rebinding has to rebuild the wrapper over
    /// `self.inner().rebind(tx)?`, which only the wrapper can do.
    fn rebind(&self, _tx: &Transaction) -> Result<Box<dyn TableShell>> {
        Err(refuse_rebind())
    }
    fn set_lock(&mut self) -> Result<()> {
        self.inner_mut().set_lock()
    }
}
