//! [`TxShell`]: the shell of a Vista wrapped in a [`Transaction`].

use std::sync::Arc;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use vantage_core::{ErrorKind, Result, VantageError};
#[cfg(feature = "rhai")]
use vantage_rhai::rhai;
use vantage_types::Record;

use super::Transaction;
use super::joining::{Joined, note_joined};
use crate::{
    Capability, Condition, NativeCondition, Page, Preview, SortDirection, TableShell, Vista,
    VistaCapabilities, VistaChangeStream, VistaMetadata, unsupported,
};

type Rec = Record<CborValue>;

/// Forwards to the shell rebound to `tx`, once `tx` is checked open. When
/// rebinding failed, `inner` is the original shell, kept for the schema
/// only, and every call answers the refusal. Writes are reported to `tx`
/// under the Vista's name, credited to the participants the rebind joined,
/// or as unprotected when it was direct. Streams (`Subscribe`, `Cursor`)
/// are hidden: they would outlive the transaction.
#[derive(Clone)]
pub(crate) struct TxShell {
    inner: Box<dyn TableShell>,
    refused: Option<Arc<VantageError>>,
    capabilities: VistaCapabilities,
    joined: Joined,
    name: String,
    tx: Transaction,
}

impl TxShell {
    pub(crate) fn new(
        original: Box<dyn TableShell>,
        rebound: Result<Box<dyn TableShell>>,
        joined: Joined,
        name: String,
        tx: Transaction,
    ) -> Self {
        let (inner, refused) = match rebound {
            Ok(rebound) => (rebound, None),
            Err(err) => (original, Some(Arc::new(err))),
        };
        let capabilities = inner
            .capabilities()
            .without(Capability::Subscribe)
            .without(Capability::Cursor);
        Self {
            inner,
            refused,
            capabilities,
            joined,
            name,
            tx,
        }
    }

    fn ready(&self) -> Result<&dyn TableShell> {
        self.tx.check_open()?;
        match &self.refused {
            Some(err) => Err(reissue(err)),
            None => Ok(&*self.inner),
        }
    }

    fn ready_mut(&mut self) -> Result<&mut dyn TableShell> {
        self.tx.check_open()?;
        match &self.refused {
            Some(err) => Err(reissue(err)),
            None => Ok(&mut *self.inner),
        }
    }

    fn written<T>(&self, out: T) -> T {
        self.tx.note_written_by(&self.name, &self.joined);
        out
    }
}

/// A fresh copy of the stored refusal, which every call answers.
fn reissue(err: &VantageError) -> VantageError {
    let fresh = VantageError::other(err.to_string());
    match err.kind() {
        ErrorKind::Unsupported => fresh.mark_unsupported(),
        ErrorKind::Unimplemented => fresh.mark_unimplemented(),
        ErrorKind::IncorrectUsage => fresh.mark_incorrect_usage(),
        ErrorKind::NotFound => fresh.mark_not_found(),
        ErrorKind::Conflict => fresh.mark_conflict(),
        ErrorKind::Generic => fresh,
    }
}

#[async_trait]
impl TableShell for TxShell {
    fn metadata(&self) -> &VistaMetadata {
        self.inner.metadata()
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
    fn driver_name(&self) -> &'static str {
        self.inner.driver_name()
    }
    fn preview(&self) -> Preview {
        self.inner.preview()
    }
    #[cfg(feature = "rhai")]
    fn register_rhai_extensions(&self, engine: &mut rhai::Engine) {
        self.inner.register_rhai_extensions(engine)
    }

    async fn list(&self) -> Result<IndexMap<String, Rec>> {
        self.ready()?.list().await
    }
    async fn get(&self, id: &str) -> Result<Option<Rec>> {
        self.ready()?.get(id).await
    }
    async fn count(&self) -> Result<i64> {
        self.ready()?.count().await
    }
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        self.ready()?.fetch_window(offset, limit).await
    }
    async fn fetch_cursor(
        &self,
        _token: Option<CborValue>,
        _limit: usize,
    ) -> Result<(Page, Option<CborValue>)> {
        self.ready()?;
        unsupported(Capability::Cursor)
    }
    async fn watch(&self) -> Result<VistaChangeStream> {
        self.ready()?;
        unsupported(Capability::Subscribe)
    }

    fn add_condition(&mut self, condition: &Condition) -> Result<()> {
        self.ready_mut()?.add_condition(condition)
    }
    fn add_native_condition(&mut self, condition: NativeCondition) -> Result<()> {
        self.ready_mut()?.add_native_condition(condition)
    }
    fn set_search(&mut self, text: Option<&str>) -> Result<()> {
        self.ready_mut()?.set_search(text)
    }
    fn set_order(&mut self, order: Option<(&str, SortDirection)>) -> Result<()> {
        self.ready_mut()?.set_order(order)
    }

    async fn insert(&self, id: Option<&str>, record: &Rec) -> Result<(String, Rec)> {
        let out = self.ready()?.insert(id, record).await?;
        Ok(self.written(out))
    }
    async fn replace(&self, id: &str, record: &Rec) -> Result<Rec> {
        let out = self.ready()?.replace(id, record).await?;
        Ok(self.written(out))
    }
    async fn upsert(&self, id: &str, record: &Rec) -> Result<Rec> {
        let out = self.ready()?.upsert(id, record).await?;
        Ok(self.written(out))
    }
    async fn patch(&self, id: &str, partial: &Rec) -> Result<Rec> {
        let out = self.ready()?.patch(id, partial).await?;
        Ok(self.written(out))
    }
    async fn delete(&self, id: &str) -> Result<()> {
        self.ready()?.delete(id).await?;
        self.written(());
        Ok(())
    }
    async fn delete_all(&self) -> Result<()> {
        self.ready()?.delete_all().await?;
        self.written(());
        Ok(())
    }
    async fn import(&self, rows: &IndexMap<String, Rec>) -> Result<usize> {
        let out = self.ready()?.import(rows).await?;
        Ok(self.written(out))
    }

    fn get_ref(&self, relation: &str, row: Option<&Rec>) -> Result<Vista> {
        let target = self.ready()?.get_ref(relation, row)?;
        Ok(self.tx.wrap(&target))
    }

    fn rebind(&self, tx: &Transaction) -> Result<Box<dyn TableShell>> {
        let inner = self.ready()?;
        if self.tx.same(tx) {
            note_joined(tx, &self.joined);
            return Ok(Box::new(self.clone()));
        }
        inner.rebind(tx)
    }
    fn set_lock(&mut self) -> Result<()> {
        self.ready_mut()?.set_lock()
    }
}
