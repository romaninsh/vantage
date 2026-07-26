//! The scalar surface: an aggregation producing a [`CborValue`] becomes a
//! [`ValueScenery`], which every existing consumer already knows how to mount.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use tokio::sync::{Notify, watch};
use tokio::task::JoinHandle;
use vantage_diorama::{CacheTable, Dio, Generation, ValueScenery, ValueStatus};
use vantage_types::Record;

use crate::engine::Publish;

/// Cache key for the single row holding a scalar aggregate's last value.
const CACHED_ROW_ID: &str = "value";
/// Field within that row.
const CACHED_FIELD: &str = "value";

pub(crate) struct ValueState {
    value: RwLock<Option<CborValue>>,
    status: RwLock<ValueStatus>,
    generation: AtomicU64,
    generation_tx: watch::Sender<Generation>,
    /// Held so `request_refresh` can push the ask *down* to the datasource.
    source: Dio,
    /// Last published value, so a restart paints before the first
    /// recomputation lands.
    cache: Arc<dyn CacheTable>,
    nudge: Arc<Notify>,
}

impl ValueState {
    pub(crate) fn new(source: Dio, cache: Arc<dyn CacheTable>, nudge: Arc<Notify>) -> Arc<Self> {
        let (generation_tx, _) = watch::channel(Generation::default());
        Arc::new(Self {
            value: RwLock::new(None),
            status: RwLock::new(ValueStatus::Loading),
            generation: AtomicU64::new(0),
            generation_tx,
            source,
            cache,
            nudge,
        })
    }

    /// Paint the cached value from a previous run, if there is one. Leaves the
    /// status `Loading`: the number is real but not yet reconfirmed.
    pub(crate) async fn seed_from_cache(&self) {
        let Ok(Some(record)) = self.cache.get_value(CACHED_ROW_ID).await else {
            return;
        };
        let Some(value) = record.get(CACHED_FIELD) else {
            return;
        };
        *self.value.write().unwrap() = Some(value.clone());
        self.bump();
    }

    fn bump(&self) {
        let next = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        // `send_replace`, not `send` — a consumer that subscribes later must
        // still observe the current generation.
        let _ = self.generation_tx.send_replace(Generation(next));
    }
}

#[async_trait]
impl Publish<CborValue> for ValueState {
    async fn publish(&self, output: CborValue) {
        let mut record = Record::new();
        record.insert(CACHED_FIELD.to_string(), output.clone());
        if let Err(e) = self.cache.insert_value(CACHED_ROW_ID, &record).await {
            // The value itself is still good; only its persistence failed.
            tracing::warn!(
                target: "vantage_diorama_aggregate",
                error = %e,
                "failed to persist aggregate value",
            );
        }
        *self.value.write().unwrap() = Some(output);
        *self.status.write().unwrap() = ValueStatus::Fresh;
        self.bump();
    }

    fn fail(&self, error: String) {
        // Keep the last good value on screen and mark it stale-with-reason.
        let mut status = self.status.write().unwrap();
        if matches!(&*status, ValueStatus::Error(existing) if existing == &error) {
            return;
        }
        *status = ValueStatus::Error(error);
        drop(status);
        self.bump();
    }
}

/// Aborts the recomputation task when the last handle drops, so a released
/// aggregate stops watching its source rather than idling for the Dio's
/// lifetime.
pub(crate) struct TaskGuard(pub(crate) JoinHandle<()>);

impl Drop for TaskGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// A scalar aggregation, mounted as a [`ValueScenery`].
pub struct AggregateValue {
    pub(crate) state: Arc<ValueState>,
    pub(crate) _guard: TaskGuard,
}

impl ValueScenery for AggregateValue {
    fn value(&self) -> Option<CborValue> {
        self.state.value.read().unwrap().clone()
    }

    fn status(&self) -> ValueStatus {
        self.state.status.read().unwrap().clone()
    }

    /// Push the ask down to the datasource rather than recomputing locally.
    ///
    /// This is the path behind an explicit "refresh" in a UI: recomputing over
    /// unchanged rows would produce the same number, so the only useful thing
    /// to do is ask the source to go and look again. The resulting events come
    /// back up the bus and drive the recomputation. The local nudge alongside
    /// it covers a source with no refresh route configured, which would
    /// otherwise leave the button doing nothing at all.
    fn request_refresh(&self) {
        let source = self.state.source.clone();
        let nudge = self.state.nudge.clone();
        tokio::spawn(async move {
            if let Err(e) = source.refresh().await {
                tracing::warn!(
                    target: "vantage_diorama_aggregate",
                    error = %e,
                    "source refresh failed",
                );
            }
            nudge.notify_one();
        });
    }

    fn subscribe(&self) -> watch::Receiver<Generation> {
        self.state.generation_tx.subscribe()
    }
}
