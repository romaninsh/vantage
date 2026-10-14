//! The scalar surface: an aggregation producing a [`CborValue`] becomes a
//! [`ValueScenery`], which every existing consumer already knows how to mount.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use tokio::sync::{Notify, watch};

use crate::aggregate::engine::{Publish, Resume};
use crate::dio::AbortOnDrop;
use crate::{Dio, Generation, ValueScenery, ValueStatus};

pub(crate) struct ValueState {
    value: RwLock<Option<CborValue>>,
    status: RwLock<ValueStatus>,
    generation: AtomicU64,
    generation_tx: watch::Sender<Generation>,
    /// Held so `refresh_source` can push the ask *down* to the datasource.
    source: Dio,
    nudge: Arc<Notify>,
}

impl ValueState {
    pub(crate) fn new(source: Dio, nudge: Arc<Notify>) -> Arc<Self> {
        let (generation_tx, _) = watch::channel(Generation::default());
        Arc::new(Self {
            value: RwLock::new(None),
            status: RwLock::new(ValueStatus::Loading),
            generation: AtomicU64::new(0),
            generation_tx,
            source,
            nudge,
        })
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
        *self.value.write().unwrap() = Some(output);
        *self.status.write().unwrap() = ValueStatus::Fresh;
        self.bump();
    }

    /// Keep the last good value on screen and mark it stale-with-reason. The
    /// next good read republishes, unchanged or not, so the status returns to
    /// `Fresh`.
    fn fail(&self, error: String) -> Resume {
        let mut status = self.status.write().unwrap();
        if !matches!(&*status, ValueStatus::Error(existing) if existing == &error) {
            *status = ValueStatus::Error(error);
            drop(status);
            self.bump();
        }
        Resume::Republish
    }
}

/// A scalar aggregation, mounted as a [`ValueScenery`]. Dropping it aborts the
/// recomputation, so a released value stops watching its source rather than
/// idling for the Dio's lifetime.
pub(crate) struct AggregateValue {
    pub(crate) state: Arc<ValueState>,
    pub(crate) _task: AbortOnDrop,
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
    fn refresh_source(&self) {
        let source = self.state.source.clone();
        let nudge = self.state.nudge.clone();
        let runtime = source.inner.lens.runtime.clone();
        runtime.spawn(async move {
            if let Err(e) = source.refresh().await {
                tracing::warn!(
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
