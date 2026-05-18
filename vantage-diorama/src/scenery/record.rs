//! `RecordScenery` — reactive single-row view.
//!
//! Opens for one record id, exposes its current cached row + status,
//! and bumps a watch channel when the underlying row changes. Status
//! transitions in v1:
//!
//! | Trigger                              | Status     |
//! |--------------------------------------|------------|
//! | Open, cache has the row              | `Fresh`    |
//! | Open, cache miss                     | `NotFound` |
//! | Reload finds the row                 | `Fresh`    |
//! | Reload finds nothing                 | `NotFound` |
//! | Reload errored (backend failure)     | `Error(_)` |
//!
//! Deliberately *not* in v1:
//!
//! - **Master fetch on cache miss** — the cache is the source of
//!   truth. Use [`Dio::patched`](crate::Dio::patched) (e.g. from an
//!   `on_query` callback once that lands in stage 5b) to seed the row.
//! - **`PendingWrite` status + `mark_pending_write`** — optimistic
//!   write tracking lands when the UI adapter (stage 8) exercises it.
//! - **`Stale` status** — no producer until refresh/TTL tracking
//!   gains a real signal.
//! - **`dirty_fields`** — the form view manages its own draft state;
//!   `EnrichedRecord` carries the slot for the future binding.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{RwLock, Weak};

use tokio::sync::{broadcast, watch};
use vantage_core::Result;
use vantage_types::Record;

use crate::dio::{DioEvent, DioInner, Generation};

use super::enriched_record::EnrichedRecord;

#[derive(Debug, Clone)]
pub enum RecordStatus {
    Fresh,
    Stale,
    Loading,
    NotFound,
    Error(String),
}

/// Reactive view onto a single record by id within a Dio.
pub trait RecordScenery: Send + Sync {
    fn record(&self) -> Option<Arc<EnrichedRecord>>;
    fn status(&self) -> RecordStatus;

    fn request_refresh(&self);
    fn subscribe(&self) -> watch::Receiver<Generation>;
}

pub(crate) struct RecordSceneryState {
    pub(crate) dio_weak: Weak<DioInner>,
    pub(crate) id: String,

    pub(crate) record: RwLock<Option<Arc<EnrichedRecord>>>,
    pub(crate) status: RwLock<RecordStatus>,

    pub(crate) generation: AtomicU64,
    pub(crate) generation_tx: watch::Sender<Generation>,
}

impl RecordSceneryState {
    /// Re-read the row from cache, update record + status, bump generation.
    async fn reload(&self) -> Result<()> {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return Ok(());
        };
        match dio_inner.cache.get_value(&self.id).await {
            Ok(Some(rec)) => self.set_loaded(rec),
            Ok(None) => self.set_not_found(),
            Err(e) => self.set_error(e.to_string()),
        }
        Ok(())
    }

    fn set_loaded(&self, rec: Record<ciborium::Value>) {
        *self.record.write().unwrap() = Some(Arc::new(EnrichedRecord::fresh(rec)));
        *self.status.write().unwrap() = RecordStatus::Fresh;
        self.bump_generation();
    }

    fn set_not_found(&self) {
        *self.record.write().unwrap() = None;
        *self.status.write().unwrap() = RecordStatus::NotFound;
        self.bump_generation();
    }

    fn set_error(&self, msg: String) {
        *self.status.write().unwrap() = RecordStatus::Error(msg);
        self.bump_generation();
    }

    fn bump_generation(&self) {
        let next = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.generation_tx.send(Generation(next));
    }
}

async fn reload_loop(state: Arc<RecordSceneryState>, mut bus: broadcast::Receiver<DioEvent>) {
    loop {
        if state.dio_weak.upgrade().is_none() {
            return;
        }
        match bus.recv().await {
            Ok(DioEvent::RecordChanged { id })
            | Ok(DioEvent::RecordInserted { id })
            | Ok(DioEvent::RecordRemoved { id })
                if id == state.id =>
            {
                if let Err(e) = state.reload().await {
                    tracing::error!(error = %e, "RecordScenery reload failed");
                }
            }
            Ok(DioEvent::Invalidated) => {
                if let Err(e) = state.reload().await {
                    tracing::error!(error = %e, "RecordScenery reload failed");
                }
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => {
                // Defensive: missed events, full reload.
                if let Err(e) = state.reload().await {
                    tracing::error!(error = %e, "RecordScenery reload failed");
                }
            }
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

pub(crate) struct RecordSceneryImpl {
    pub(crate) inner: Arc<RecordSceneryState>,
}

impl RecordScenery for RecordSceneryImpl {
    fn record(&self) -> Option<Arc<EnrichedRecord>> {
        self.inner.record.read().unwrap().clone()
    }

    fn status(&self) -> RecordStatus {
        self.inner.status.read().unwrap().clone()
    }

    fn request_refresh(&self) {
        let Some(dio_inner) = self.inner.dio_weak.upgrade() else {
            return;
        };
        let runtime = dio_inner.lens.runtime.clone();
        runtime.spawn(async move {
            let dio = crate::Dio { inner: dio_inner };
            if let Err(e) = dio.refresh().await {
                tracing::error!(error = %e, "RecordScenery request_refresh failed");
            }
            // refresh() publishes `Invalidated`; the bus task reloads.
        });
    }

    fn subscribe(&self) -> watch::Receiver<Generation> {
        self.inner.generation_tx.subscribe()
    }
}

/// Internal constructor — wires the bus task and returns the impl.
/// Used by [`Dio::record_scenery`](crate::Dio::record_scenery) and
/// [`Dio::record_scenery_with`](crate::Dio::record_scenery_with).
pub(crate) fn spawn_record_scenery(
    dio: &Arc<DioInner>,
    id: String,
    initial_record: Option<Record<ciborium::Value>>,
    initial_status: RecordStatus,
) -> Arc<dyn RecordScenery> {
    let (gen_tx, _gen_rx) = watch::channel(Generation::default());
    let state = Arc::new(RecordSceneryState {
        dio_weak: Arc::downgrade(dio),
        id,
        record: RwLock::new(initial_record.map(|r| Arc::new(EnrichedRecord::fresh(r)))),
        status: RwLock::new(initial_status),
        generation: AtomicU64::new(0),
        generation_tx: gen_tx,
    });

    let bus_rx = dio.event_bus.subscribe();
    let task_state = state.clone();
    dio.lens.runtime.spawn(async move {
        reload_loop(task_state, bus_rx).await;
    });

    Arc::new(RecordSceneryImpl { inner: state }) as Arc<dyn RecordScenery>
}
