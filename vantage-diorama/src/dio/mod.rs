pub mod event_bus;
pub mod hot_tier;
pub mod impls;
pub(crate) mod query_index;
pub mod refresh;
pub mod shell;
pub mod worker;

use std::sync::Arc;

use tokio::sync::{Mutex, broadcast, mpsc};
use tokio::task::JoinHandle;
use vantage_core::Result;
use vantage_vista::Vista;

use crate::lens::{CacheTable, Lens};
use crate::ops::{ChangeEvent, WriteOp};
use crate::scenery::record::spawn_record_scenery;
use crate::scenery::{
    RecordScenery, RecordStatus, TableScenery, TableSceneryBuilder, ValueSceneryBuilder,
};

use ciborium::Value as CborValue;
use vantage_types::Record;

pub use event_bus::DioEvent;
pub use hot_tier::HotTier;
pub use shell::DioShell;

/// Monotonically-increasing per-Scenery counter. Bumped on every state
/// change a Scenery exposes; UI adapters watch the receiver and
/// re-render on each bump.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Generation(pub u64);

impl From<u64> for Generation {
    fn from(v: u64) -> Self {
        Generation(v)
    }
}

impl From<Generation> for u64 {
    fn from(g: Generation) -> Self {
        g.0
    }
}

/// Per-entity binding of a Vista to a Lens.
///
/// Cheap to clone — wraps an `Arc<DioInner>` so all clones share the
/// same write queue, event bus, refresh task, and hot tier. Sceneries
/// keep their own `Arc<DioInner>` and remain alive as long as any
/// handle outlives the original Dio.
#[derive(Clone)]
pub struct Dio {
    pub(crate) inner: Arc<DioInner>,
}

pub(crate) struct DioInner {
    pub(crate) lens: Arc<Lens>,
    pub(crate) master: Vista,
    pub(crate) cache: Arc<dyn CacheTable>,
    pub(crate) cache_table_name: String,
    pub(crate) write_queue: mpsc::Sender<WriteOp>,
    pub(crate) event_bus: broadcast::Sender<DioEvent>,
    pub(crate) refresh_task: Mutex<Option<JoinHandle<()>>>,
    pub(crate) write_worker: Mutex<Option<JoinHandle<()>>>,
    pub(crate) hot_tier: Arc<HotTier>,
    /// Per-query ordered indexes, keyed by [`Vista::index_key`]. Shared across
    /// every two-pass scenery of this Dio so reopening the same filter/sort
    /// reuses the already-built index. Not persisted — re-listing rebuilds it.
    pub(crate) query_indexes: std::sync::Mutex<
        std::collections::HashMap<String, Arc<crate::dio::query_index::QueryIndex>>,
    >,
    /// Deduplicating registry of live table sceneries, keyed by
    /// `(shape, conditions, sort, search)`. Holds `Weak` handles so it
    /// never keeps a scenery alive: opening the same query twice returns
    /// the one shared `Arc` (one reactor, one cache window, one in-flight
    /// `JoinSet`), and the entry self-heals once the last widget releases
    /// it. This is what makes "scenery must be cheap" true and what lets a
    /// closing grid stop pulling — see [`TableSceneryImpl`]'s drop guard.
    pub(crate) table_sceneries:
        std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<dyn TableScenery>>>,
}

impl DioInner {
    /// Fetch (or lazily create) the [`QueryIndex`](crate::dio::query_index::QueryIndex)
    /// for `key`. Repeated calls with the same key return the same `Arc`, so
    /// all sceneries on a query variant share one ordered index.
    pub(crate) fn query_index(&self, key: &str) -> Arc<crate::dio::query_index::QueryIndex> {
        let mut guard = self.query_indexes.lock().unwrap();
        guard
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(crate::dio::query_index::QueryIndex::new()))
            .clone()
    }

    /// Return the live shared table scenery for `key`, or `None` if none is
    /// open (or the last handle was just released — a dead `Weak`).
    pub(crate) fn lookup_table_scenery(&self, key: &str) -> Option<Arc<dyn TableScenery>> {
        self.table_sceneries
            .lock()
            .unwrap()
            .get(key)
            .and_then(std::sync::Weak::upgrade)
    }

    /// Publish a freshly-built scenery under `key`. If a concurrent open won
    /// the race for the same key, returns that shared scenery instead and lets
    /// `built` drop — its guard aborts the now-redundant tasks. Otherwise
    /// inserts a `Weak` to `built` and hands it back.
    pub(crate) fn register_table_scenery(
        &self,
        key: String,
        built: Arc<dyn TableScenery>,
    ) -> Arc<dyn TableScenery> {
        let mut guard = self.table_sceneries.lock().unwrap();
        if let Some(existing) = guard.get(&key).and_then(std::sync::Weak::upgrade) {
            return existing;
        }
        guard.insert(key, Arc::downgrade(&built));
        built
    }
}

impl Dio {
    pub fn master(&self) -> &Vista {
        &self.inner.master
    }

    pub fn cache(&self) -> &Arc<dyn CacheTable> {
        &self.inner.cache
    }

    pub fn cache_table_name(&self) -> &str {
        &self.inner.cache_table_name
    }

    /// Subscribe to the Dio's internal event bus. Sceneries call this
    /// in their `subscribe` impl; user callbacks may also call it to
    /// observe cross-Dio reactions.
    pub fn subscribe_events(&self) -> broadcast::Receiver<DioEvent> {
        self.inner.event_bus.subscribe()
    }

    /// Take the per-Dio write worker's `JoinHandle` out of the inner
    /// state. Returns `Some` on the first call, `None` afterwards.
    ///
    /// Once taken, the worker is no longer owned by the Dio — it keeps
    /// running until the last `Sender` (held by `DioInner`) drops, at
    /// which point the loop's `recv()` returns `None` and the task
    /// completes. Callers can `await` the returned handle to observe
    /// that clean shutdown.
    ///
    /// Intended for test harnesses asserting worker lifecycle; not part
    /// of the standard surface.
    #[doc(hidden)]
    pub async fn take_write_worker_handle(&self) -> Option<JoinHandle<()>> {
        self.inner.write_worker.lock().await.take()
    }

    /// Start a [`TableScenery`](crate::scenery::TableScenery) builder
    /// for this Dio. Chainable; call `.open().await` to spawn the
    /// reactive view.
    pub fn table_scenery(&self) -> TableSceneryBuilder {
        TableSceneryBuilder::new(self.inner.clone())
    }

    /// Number of distinct table sceneries currently held open on this Dio.
    ///
    /// Prunes dead registry entries as a side effect, so the count reflects
    /// only sceneries with at least one live handle. Two widgets sharing one
    /// deduplicated `(conditions, sort, search)` count as **one**; once every
    /// handle is released the count drops back, proving no leak. A read-only
    /// window onto the dedup registry — the seed for the diagnostics surface.
    pub fn live_table_scenery_count(&self) -> usize {
        let mut guard = self.inner.table_sceneries.lock().unwrap();
        guard.retain(|_, weak| weak.strong_count() > 0);
        guard.len()
    }

    /// Open a reactive view onto a single record by id. Reads the
    /// cache once at creation:
    ///
    /// - cache hit → `RecordStatus::Fresh`, record exposed
    /// - cache miss → `RecordStatus::NotFound`, record = `None`
    ///
    /// No master fetch on miss (the cache is the source of truth in
    /// v1). Use [`Dio::patched`](Self::patched) — from an `on_query`
    /// callback or your own code — to seed the row.
    pub async fn record_scenery(&self, id: impl Into<String>) -> Result<Arc<dyn RecordScenery>> {
        let id = id.into();
        let (initial_record, initial_status) = match self.inner.cache.get_value(&id).await? {
            Some(rec) => (Some(rec), RecordStatus::Fresh),
            None => (None, RecordStatus::NotFound),
        };
        Ok(spawn_record_scenery(
            &self.inner,
            id,
            initial_record,
            initial_status,
        ))
    }

    /// Open a reactive view onto a single record with the row already
    /// in hand — the parent grid hands its current row off to the
    /// detail view without a cache round-trip. Status is `Fresh`.
    pub fn record_scenery_with(
        &self,
        id: impl Into<String>,
        record: Record<CborValue>,
    ) -> Arc<dyn RecordScenery> {
        spawn_record_scenery(&self.inner, id.into(), Some(record), RecordStatus::Fresh)
    }

    /// Start a [`ValueScenery`](crate::scenery::ValueScenery) builder.
    /// Chain `.count()` / `.sum(col)` / `.custom(closure)` /
    /// `.aggregate(...)`, then `.open().await`.
    pub fn value_scenery(&self) -> ValueSceneryBuilder {
        ValueSceneryBuilder::new(self.inner.clone())
    }

    /// Produce a fresh facade [`Vista`] backed by this Dio. Each call
    /// returns an independent Vista — callers can narrow with
    /// [`Vista::add_condition_eq`] without affecting other consumers.
    ///
    /// The facade's schema mirrors `master` (forwarded through
    /// [`DioShell`]'s [`columns`](vantage_vista::TableShell::columns)
    /// etc.) while reads route through the cache and writes route
    /// through the Dio's queue.
    pub fn vista(&self) -> Vista {
        let name = self.inner.master.name().to_string();
        let shell = DioShell::new(self.inner.clone());
        Vista::new(name, Box::new(shell))
    }

    // ---- Event bus — user-callable surface ----------------------------------

    /// Dispatch an upstream [`ChangeEvent`] through the lens's
    /// `on_event` callback. Returns `Ok(())` immediately when no
    /// `on_event` is registered.
    ///
    /// This is the entry point for live-stream forwarders: the user
    /// `tokio::spawn`s a task that pumps events from a
    /// `LiveStream`/`broadcast::Receiver`/channel into
    /// `dio.handle_event(evt).await`. The callback decides how to
    /// reconcile cache state and publish bus events (typically via
    /// [`patched`](Self::patched) or [`invalidate_record`](Self::invalidate_record)).
    pub async fn handle_event(&self, evt: ChangeEvent) -> Result<()> {
        if let Some(cb) = self.inner.lens.callbacks.on_event.as_ref() {
            cb(self, evt).await
        } else {
            Ok(())
        }
    }

    /// Publish [`DioEvent::RecordChanged`] on the bus. Doesn't touch
    /// the cache — use [`patched`](Self::patched) when you also have
    /// the new record value.
    pub fn invalidate_record(&self, id: impl Into<String>) {
        let _ = self
            .inner
            .event_bus
            .send(DioEvent::RecordChanged { id: id.into() });
    }

    /// Publish [`DioEvent::Invalidated`] on the bus. Sceneries respond
    /// by re-reading their full state.
    pub fn invalidate_all(&self) {
        let _ = self.inner.event_bus.send(DioEvent::Invalidated);
    }

    /// Write `record` to the cache under `id` and publish
    /// [`DioEvent::RecordChanged`]. The canonical "external system
    /// told us about a row" pattern inside an `on_event` callback.
    pub async fn patched(&self, id: impl Into<String>, record: Record<CborValue>) -> Result<()> {
        let id = id.into();
        self.inner.cache.insert_value(&id, &record).await?;
        let _ = self.inner.event_bus.send(DioEvent::RecordChanged { id });
        Ok(())
    }

    /// Remove `id` from the cache and publish [`DioEvent::RecordRemoved`].
    /// Symmetric to [`patched`](Self::patched) — call after a successful
    /// master-side delete so subscribed Sceneries drop the row from
    /// their view. Without the cache wipe, the bus event still fires
    /// but Sceneries that reseed from the cache (e.g. TableScenery)
    /// re-include the row, leaving the grid out of sync with the
    /// master until the next `refresh()` / `invalidate_all()`.
    ///
    /// `Ok(())` if the row wasn't in the cache to begin with —
    /// idempotent.
    pub async fn removed(&self, id: impl Into<String>) -> Result<()> {
        let id = id.into();
        self.inner.cache.delete_value(&id).await?;
        let _ = self.inner.event_bus.send(DioEvent::RecordRemoved { id });
        Ok(())
    }

    /// Fire the `on_refresh` callback synchronously. Errors propagate
    /// to the caller (the scheduled refresh task only logs them).
    ///
    /// Returns `Ok(())` immediately when no `on_refresh` is registered.
    pub async fn refresh(&self) -> Result<()> {
        let _ = self.inner.event_bus.send(DioEvent::Refreshing);
        let result = if let Some(cb) = self.inner.lens.callbacks.on_refresh.as_ref() {
            cb(self).await
        } else {
            Ok(())
        };
        if result.is_ok() {
            let _ = self.inner.event_bus.send(DioEvent::Invalidated);
        }
        result
    }
}
