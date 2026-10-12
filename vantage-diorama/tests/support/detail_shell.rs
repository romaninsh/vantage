//! A get-only detail shell for augmentation tests: it lists nothing, takes no
//! narrowing, and answers `get` from fixed rows. Every `get` is counted and
//! logged in dispatch order, and waits for a permit from `gate`, so a test can
//! hold a fetch mid-flight.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use indexmap::IndexMap;
use tokio::sync::Semaphore;
use vantage_core::{Result, error};
use vantage_types::Record;
use vantage_vista::{
    Column, Condition, Page, Preview, TableShell, VistaCapabilities, VistaMetadata,
};

#[derive(Clone)]
pub struct DetailShell {
    rows: Arc<IndexMap<String, Record<CborValue>>>,
    /// How many `get`s were made.
    pub gets: Arc<AtomicUsize>,
    /// Ids in the order the `get`s were dispatched.
    pub log: Arc<Mutex<Vec<String>>>,
    /// Fetch admission: each `get` consumes one permit.
    pub gate: Arc<Semaphore>,
    /// The most `get`s ever in flight at once.
    pub max_concurrent: Arc<AtomicUsize>,
    concurrent: Arc<AtomicUsize>,
    metadata: VistaMetadata,
    capabilities: VistaCapabilities,
}

impl DetailShell {
    /// A shell over `rows` (columns `id`, `size`) whose `get`s never wait.
    pub fn new(rows: IndexMap<String, Record<CborValue>>) -> Self {
        Self::with_permits(rows, Semaphore::MAX_PERMITS)
    }

    /// Like [`new`](Self::new), with the gate closed: no `get` returns until
    /// the test calls `gate.add_permits`.
    pub fn gated(rows: IndexMap<String, Record<CborValue>>) -> Self {
        Self::with_permits(rows, 0)
    }

    fn with_permits(rows: IndexMap<String, Record<CborValue>>, permits: usize) -> Self {
        let metadata = VistaMetadata::new()
            .with_column(Column::new("id", "String").with_flag("id"))
            .with_column(Column::new("size", "String"))
            .with_id_column("id");
        Self {
            rows: Arc::new(rows),
            gets: Arc::new(AtomicUsize::new(0)),
            log: Arc::new(Mutex::new(Vec::new())),
            gate: Arc::new(Semaphore::new(permits)),
            max_concurrent: Arc::new(AtomicUsize::new(0)),
            concurrent: Arc::new(AtomicUsize::new(0)),
            metadata,
            capabilities: VistaCapabilities::new(),
        }
    }
}

#[async_trait]
impl TableShell for DetailShell {
    fn metadata(&self) -> &VistaMetadata {
        &self.metadata
    }
    fn capabilities(&self) -> &VistaCapabilities {
        &self.capabilities
    }
    fn driver_name(&self) -> &'static str {
        "detail"
    }
    fn preview(&self) -> Preview {
        Preview::new()
    }
    async fn list(&self) -> Result<IndexMap<String, Record<CborValue>>> {
        Ok(IndexMap::new())
    }
    async fn get(&self, id: &str) -> Result<Option<Record<CborValue>>> {
        self.gets.fetch_add(1, Ordering::SeqCst);
        let now = self.concurrent.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_concurrent.fetch_max(now, Ordering::SeqCst);
        self.log.lock().unwrap().push(id.to_string());
        let permit = self.gate.acquire().await.expect("gate open");
        permit.forget(); // each permit admits exactly one fetch
        self.concurrent.fetch_sub(1, Ordering::SeqCst);
        Ok(self.rows.get(id).cloned())
    }
    async fn count(&self) -> Result<i64> {
        Ok(0)
    }
    async fn fetch_window(&self, offset: usize, limit: usize) -> Result<Page> {
        Ok(Page::from_listing(IndexMap::new(), offset, limit))
    }
    fn add_condition(&mut self, _condition: &Condition) -> Result<()> {
        Err(error!("a get-only detail shell takes no conditions").mark_unimplemented())
    }
}
