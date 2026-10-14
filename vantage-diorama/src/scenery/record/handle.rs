//! The handle a record scenery is held by, and the bus task it owns.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock, Weak};

use tokio::sync::{broadcast, watch};

use super::{RecordScenery, RecordSceneryState, RecordStatus};
use crate::dio::{DioEvent, DioInner, Generation};
use crate::lens::cache_backend::CacheTable;
use crate::scenery::enriched_record::EnrichedRecord;

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
    /// Weak — the guard must not keep the Dio alive; it only reaches back
    /// in to decrement the census and emit the closing line.
    dio_weak: Weak<DioInner>,
}

impl Drop for RecordSceneryGuard {
    fn drop(&mut self) {
        self.task.abort();
        if let Some(dio) = self.dio_weak.upgrade() {
            dio.record_census.fetch_sub(1, Ordering::Relaxed);
            dio.emit_census("record scenery", "closed");
        }
    }
}

impl RecordScenery for RecordSceneryImpl {
    fn record(&self) -> Option<Arc<EnrichedRecord>> {
        self.inner.record.read().unwrap().clone()
    }

    fn status(&self) -> RecordStatus {
        self.inner.status.read().unwrap().clone()
    }

    fn refresh_source(&self) {
        let Some(dio_inner) = self.inner.dio_weak.upgrade() else {
            return;
        };
        let runtime = dio_inner.lens.runtime.clone();
        runtime.spawn(async move {
            let dio = crate::Dio { inner: dio_inner };
            if let Err(e) = dio.refresh().await {
                tracing::error!(error = %e, "RecordScenery refresh_source failed");
            }
            // refresh() publishes `DatasetChanged`; the bus task reloads.
        });
    }

    fn subscribe(&self) -> watch::Receiver<Generation> {
        self.inner.generation_tx.subscribe()
    }
}

impl RecordSceneryState {
    /// Show the optimistically-staged value while the write is in flight.
    async fn set_pending_write(&self) {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return;
        };
        match self.reader(&dio_inner).get_value(&self.id).await {
            Ok(Some(rec)) => self.publish(|shown, _| {
                *shown = Some(Arc::new(EnrichedRecord::pending_write(rec)));
            }),
            // Nothing staged under this id (an id-less insert): nothing to show.
            Ok(None) => {}
            Err(e) => self.set_error(e.to_string()),
        }
    }

    /// The optimistic write was rolled back: re-read the restored pre-image and
    /// flag the failure so the form can surface it.
    async fn set_write_failed(&self, error: String) {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return;
        };
        let restored = match self.reader(&dio_inner).get_value(&self.id).await {
            Ok(Some(rec)) => Some(Arc::new(EnrichedRecord::write_failed(rec, error))),
            // Pre-image was absent (a failed insert): drop the row.
            Ok(None) => None,
            // The restored row can't be read: keep what is shown, and say
            // both why the write failed and why the row isn't current.
            Err(e) => {
                return self.set_error(format!("{error}; reading the restored row failed: {e}"));
            }
        };
        self.publish(|shown, _| *shown = restored);
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
                state.reload().await
            }
            // `Seeded` alongside `DatasetChanged`: a record scenery opened over
            // a cold cache found nothing, and the eager seed landing is the one
            // notice it gets that its record now exists.
            Ok(DioEvent::DatasetChanged) | Ok(DioEvent::Seeded) => state.reload().await,
            Ok(DioEvent::RecordLoadFailed { id, error }) if id == state.id => {
                state.set_detail_failed(error)
            }
            Ok(DioEvent::WritePending { id, .. }) if id == state.id => {
                state.set_pending_write().await;
            }
            Ok(DioEvent::WriteReverted { id, error, .. }) if id == state.id => {
                state.set_write_failed(error).await;
            }
            Ok(_) => {}
            // Defensive: missed events, full reload.
            Err(broadcast::error::RecvError::Lagged(_)) => state.reload().await,
            Err(broadcast::error::RecvError::Closed) => return,
        }
    }
}

/// Wire the bus task and return the view over `id`, showing nothing yet
/// with status `Loading`; the caller sets what it opens on. `fetching` is
/// set before the bus task can reload, so a reload while the open still may
/// fetch keeps `Loading`.
pub(crate) fn spawn_record_scenery(
    dio: &Arc<DioInner>,
    id: String,
    read_cache: Option<Arc<dyn CacheTable>>,
    fetching: bool,
) -> RecordSceneryImpl {
    let (gen_tx, _gen_rx) = watch::channel(Generation::default());
    let state = Arc::new(RecordSceneryState {
        _tally: crate::stats::Tally::record_scenery(),
        dio_weak: Arc::downgrade(dio),
        id,
        read_cache,
        record: RwLock::new(None),
        status: RwLock::new(RecordStatus::Loading),
        fetching: AtomicBool::new(fetching),
        hydration: OnceLock::new(),
        generation: AtomicU64::new(0),
        generation_tx: gen_tx,
    });

    let bus_rx = dio.event_bus.subscribe();
    let task = dio.lens.runtime.spawn(reload_loop(state.clone(), bus_rx));

    dio.record_census.fetch_add(1, Ordering::Relaxed);
    dio.emit_census("record scenery", "opened");

    RecordSceneryImpl {
        inner: state,
        _guard: RecordSceneryGuard {
            task,
            dio_weak: Arc::downgrade(dio),
        },
    }
}
