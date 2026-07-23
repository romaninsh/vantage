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
//!   truth. Use [`Dio::patched`](crate::Dio::patched) to seed the row.
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
    /// Live-instance census (see [`crate::stats`]).
    pub(crate) _tally: crate::stats::Tally,
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

    /// Show the optimistically-staged value while the write is in flight.
    async fn set_pending_write(&self) {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return;
        };
        if let Ok(Some(rec)) = dio_inner.cache.get_value(&self.id).await {
            *self.record.write().unwrap() = Some(Arc::new(EnrichedRecord::pending_write(rec)));
            self.bump_generation();
        }
    }

    /// The optimistic write was rolled back: re-read the restored pre-image and
    /// flag the failure so the form can surface it.
    async fn set_write_failed(&self, error: String) {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return;
        };
        match dio_inner.cache.get_value(&self.id).await {
            Ok(Some(rec)) => {
                *self.record.write().unwrap() =
                    Some(Arc::new(EnrichedRecord::write_failed(rec, error)));
            }
            // Pre-image was absent (a failed insert): drop the row.
            _ => *self.record.write().unwrap() = None,
        }
        self.bump_generation();
    }

    fn bump_generation(&self) {
        let next = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        // `send_replace` (not `send`) — the stored value must reflect the
        // current generation even when there are momentarily zero
        // receivers. UIs that drop and re-subscribe must see the latest.
        let _ = self.generation_tx.send_replace(Generation(next));
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
            Ok(DioEvent::DatasetChanged) => {
                if let Err(e) = state.reload().await {
                    tracing::error!(error = %e, "RecordScenery reload failed");
                }
            }
            Ok(DioEvent::WritePending { id, .. }) if id == state.id => {
                state.set_pending_write().await;
            }
            Ok(DioEvent::WriteReverted { id, error, .. }) if id == state.id => {
                state.set_write_failed(error).await;
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
    /// Aborts the bus task when the last handle drops — a released record
    /// view stops reacting instead of living for the Dio's whole lifetime
    /// (one leaked task per view would add up fast for per-request views,
    /// e.g. an HTTP watch endpoint opening one per connection).
    _guard: RecordSceneryGuard,
}

struct RecordSceneryGuard {
    task: tokio::task::JoinHandle<()>,
}

impl Drop for RecordSceneryGuard {
    fn drop(&mut self) {
        self.task.abort();
    }
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
            // refresh() publishes `DatasetChanged`; the bus task reloads.
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
        _tally: crate::stats::Tally::record_scenery(),
        dio_weak: Arc::downgrade(dio),
        id,
        record: RwLock::new(initial_record.map(|r| Arc::new(EnrichedRecord::fresh(r)))),
        status: RwLock::new(initial_status),
        generation: AtomicU64::new(0),
        generation_tx: gen_tx,
    });

    let bus_rx = dio.event_bus.subscribe();
    let task_state = state.clone();
    let task = dio.lens.runtime.spawn(async move {
        reload_loop(task_state, bus_rx).await;
    });

    Arc::new(RecordSceneryImpl {
        inner: state,
        _guard: RecordSceneryGuard { task },
    }) as Arc<dyn RecordScenery>
}
