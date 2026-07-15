pub(crate) mod augment_passes;
pub(crate) mod augment_scheduler;
pub mod diagnostics;
pub mod event_bus;
pub mod hot_tier;
pub mod impls;
mod optimistic;
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

/// Stringify a scalar CBOR id for use inside a cache table name. Non-scalars
/// yield an empty string (the name then degrades to the shared, id-less form).
fn cbor_scalar_string(v: &CborValue) -> String {
    match v {
        CborValue::Text(s) => s.clone(),
        CborValue::Integer(i) => i128::from(*i).to_string(),
        CborValue::Bool(b) => b.to_string(),
        CborValue::Float(f) => f.to_string(),
        _ => String::new(),
    }
}

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
    /// The master Vista, swappable so a [`reload`](Dio::reload) can re-point the
    /// Dio at a freshly-built Vista (e.g. after its VistaFactory reloaded)
    /// without tearing the Dio down. Read via [`Dio::master`].
    pub(crate) master: std::sync::RwLock<Arc<Vista>>,
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
    /// Dio-level query semantics inherited by every scenery opened on this Dio.
    /// The Dio — not the scenery — defines *what this table is*: a base set of
    /// equality conditions and an optional default order. A scenery may layer
    /// further per-view conditions/sort on top (see `TableSceneryBuilder`).
    pub(crate) base_conditions: std::sync::RwLock<Vec<(String, CborValue)>>,
    pub(crate) base_sort: std::sync::RwLock<Option<(String, crate::scenery::SortDir)>>,
    /// Augmentation owned by the Dio (not the Lens). When set, the Dio drives its
    /// own two-pass list/detail/refresh from this config (see `augment_passes`);
    /// `augmented_columns` is the union of every augmentation's merged columns,
    /// used to route conditions/sort on a client-side column to local emulation.
    pub(crate) augmentations: std::sync::RwLock<Option<Arc<Vec<crate::augment::Augmentation>>>>,
    pub(crate) augment_catalog: std::sync::RwLock<Option<Arc<vantage_vista_factory::VistaCatalog>>>,
    pub(crate) augmented_columns: std::sync::RwLock<std::collections::HashSet<String>>,
    /// Deduplicating registry of live table sceneries, keyed by
    /// `(shape, conditions, sort, search)`. Holds `Weak` handles so it
    /// never keeps a scenery alive: opening the same query twice returns
    /// the one shared `Arc` (one reactor, one cache window, one in-flight
    /// `JoinSet`), and the entry self-heals once the last widget releases
    /// it. This is what makes "scenery must be cheap" true and what lets a
    /// closing grid stop pulling — see `TableSceneryImpl`'s drop guard.
    pub(crate) table_sceneries:
        std::sync::Mutex<std::collections::HashMap<String, std::sync::Weak<dyn TableScenery>>>,
    /// Central per-row detail-fetch scheduler: every consumer (scenery
    /// viewport, blocking facade read) queues ids here and a small worker
    /// pool drains them — one flight per row, round-robin across consumers.
    /// Inert (no tasks) until [`ensure_augment_workers`](Self::ensure_augment_workers) runs.
    pub(crate) augment_scheduler: Arc<augment_scheduler::AugmentScheduler>,
    /// Worker tasks spawned by `ensure_augment_workers`, aborted when the
    /// Dio drops — a parked worker holds only a `Weak` and would otherwise
    /// idle forever.
    pub(crate) augment_worker_handles: std::sync::Mutex<Vec<JoinHandle<()>>>,
}

impl Drop for DioInner {
    fn drop(&mut self) {
        for handle in self.augment_worker_handles.lock().unwrap().drain(..) {
            handle.abort();
        }
    }
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

    /// Whether this Dio owns an augmentation config (drives its own two-pass).
    pub(crate) fn has_dio_augment(&self) -> bool {
        self.augmentations.read().unwrap().is_some()
    }

    /// Whether this Dio engages two-pass loading — either it owns augmentation
    /// or its Lens registers an explicit `on_load_detail` callback (hand-rolled
    /// two-pass).
    pub(crate) fn is_two_pass(&self) -> bool {
        self.has_dio_augment() || self.lens.callbacks.on_load_detail.is_some()
    }

    /// Whether `col` is produced by augmentation (a client-side column the master
    /// can't filter/sort on) — the signal to route its conditions/sort to local
    /// emulation. Always `false` until [`Dio::augment`] populates the set.
    pub(crate) fn is_augmented_column(&self, col: &str) -> bool {
        self.augmented_columns.read().unwrap().contains(col)
    }

    /// The union of every LIVE open scenery's demanded columns — the Dio's
    /// active demand. `None` = at least one open view demands everything (or
    /// no view declared a demand): the pre-demand behavior. `Some(union)` =
    /// only these columns are looked at; the augment detail pass runs only
    /// while the union intersects the augment columns. Recomputed on the fly
    /// from the dedup registry's live entries, so demand follows scenery
    /// open/close with no extra lifecycle machinery.
    pub(crate) fn demanded_columns(&self) -> Option<std::collections::HashSet<String>> {
        let mut union = std::collections::HashSet::new();
        let mut any_live = false;
        let guard = self.table_sceneries.lock().unwrap();
        for weak in guard.values() {
            let Some(scenery) = weak.upgrade() else {
                continue;
            };
            any_live = true;
            match scenery.demanded_columns() {
                None => return None,
                Some(columns) => union.extend(columns),
            }
        }
        // No live views at all: stay permissive (a detail pass mid-flight on
        // a closing scenery must not stall on an empty union).
        if !any_live { None } else { Some(union) }
    }

    /// Whether the augment detail pass has work under the current demand:
    /// true when this Dio's augment columns intersect the open sceneries'
    /// demand union (or when either side is un-enumerable).
    pub(crate) fn augment_demanded(&self) -> bool {
        let augmented = self.augmented_columns.read().unwrap();
        if augmented.is_empty() {
            return true; // un-enumerable augment ("lift all"): always demanded
        }
        match self.demanded_columns() {
            None => true,
            Some(demanded) => augmented.iter().any(|c| demanded.contains(c)),
        }
    }

    /// Spawn the augment-scheduler worker pool if it isn't running yet.
    /// Idempotent and cheap after the first call. Called wherever two-pass
    /// hydration can first be needed — [`Dio::augment`], a two-pass scenery
    /// open, a facade read about to block on hydration — so a Dio that never
    /// hydrates never runs a worker task.
    pub(crate) fn ensure_augment_workers(self: &Arc<Self>) {
        let mut guard = self.augment_worker_handles.lock().unwrap();
        if !guard.is_empty() {
            return;
        }
        let workers = self.lens.defaults.augment_workers.max(1);
        for _ in 0..workers {
            let weak = Arc::downgrade(self);
            let scheduler = self.augment_scheduler.clone();
            guard.push(
                self.lens
                    .runtime
                    .spawn(augment_scheduler::augment_worker_loop(weak, scheduler)),
            );
        }
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
    /// The current master Vista (cloned `Arc`). Cheap; safe to hold across
    /// awaits even while a concurrent [`reload`](Self::reload) swaps it.
    pub fn master(&self) -> Arc<Vista> {
        self.inner.master.read().unwrap().clone()
    }

    /// Traverse a reference and return a NEW [`Dio`] bound to the traversed
    /// target Vista — mirroring `Table::get_ref` → `Table` and
    /// [`Vista::get_ref`] → `Vista`. The new Dio reuses this Dio's [`Lens`], so
    /// the target loads through the same cache-first, failure-tolerant path:
    /// a temporarily-unreachable target yields an empty/stale-but-recovering
    /// scenery, never a hard error. The ONLY failure here is a structural one —
    /// the reference is undefined or the parent row lacks the join field —
    /// surfaced synchronously by the underlying `Vista::get_ref`.
    ///
    /// Dio is persistence-agnostic: it delegates resolution to the master
    /// Vista's `get_ref` and wraps whatever Vista comes back.
    pub async fn get_ref(&self, relation: &str, row: &Record<CborValue>) -> Result<Dio> {
        // Resolve the target Vista — pure descriptor work delegated to the
        // master shell. The only failure is structural (undefined relation /
        // missing join field); a down *source* does not fail here, it surfaces
        // later as an empty/recovering scenery on the returned Dio.
        let target = self.master().get_ref(relation, row)?;

        // Per-parent cache identity. A narrowed target (e.g. `crew` for launch
        // L1 vs L2 — both `name()` "launch_crew") must NOT share one cache
        // table, or one parent's snapshot refresh would clobber the other's.
        // `Vista` doesn't expose its conditions, but we know the relation and
        // the parent row, so derive the key the way the UI's detail tabs do:
        // `{target}-via-{relation}-{parent_id}`.
        let parent_id = self
            .master()
            .get_id_column()
            .and_then(|idc| row.get(idc))
            .map(cbor_scalar_string)
            .unwrap_or_default();
        let cache_table_name = format!("{}-via-{}-{}", target.name(), relation, parent_id);

        self.inner.lens.make_dio_as(target, cache_table_name).await
    }

    /// Re-point this Dio at a freshly-built master Vista and rebuild its cache
    /// from it — the "its VistaFactory reloaded, the dataset may be wholly
    /// different" path. The swap is **non-blanking**: open sceneries keep
    /// showing their current rows until the cache is refilled, then soft-reseed
    /// in one atomic swap on the trailing `DatasetChanged`. Stale per-query
    /// indexes are dropped so two-pass orders rebuild against the new data.
    pub async fn reload(&self, new_master: Vista) -> Result<()> {
        *self.inner.master.write().unwrap() = Arc::new(new_master);
        self.inner.query_indexes.lock().unwrap().clear();

        // Refill the cache from the new master. The cache is briefly empty
        // here — so we deliberately do NOT emit `Refreshing` (which an eager
        // scenery would reseed on, blanking to the empty cache). No scenery
        // reseeds until the single `DatasetChanged` below, by which point the new
        // data is staged; open sceneries keep their old rows visible until then
        // and swap in one atomic step, so nothing blanks.
        self.inner.cache.clear().await?;
        if let Some(on_start) = self.inner.lens.callbacks.on_start.as_ref() {
            on_start(self).await?;
        } else if let Some(on_refresh) = self.inner.lens.callbacks.on_refresh.as_ref() {
            on_refresh(self).await?;
        }
        let _ = self.inner.event_bus.send(DioEvent::DatasetChanged);
        Ok(())
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

    /// Add a base equality condition that every scenery on this Dio inherits.
    ///
    /// The Dio owns query semantics: a condition set here defines *what this
    /// table is* (e.g. "the John-filtered collection"), so all views — grids,
    /// pickers, detail panes — see the same narrowed dataset. A scenery may add
    /// further per-view conditions on top via
    /// [`where_eq`](crate::scenery::TableSceneryBuilder::where_eq).
    ///
    /// How the condition is honoured depends on the column: a native column on
    /// a capable master pushes down; a column the master can't filter (or an
    /// augmented one) is emulated locally over the cache. Returns a clone so
    /// calls chain. Conditions take effect for sceneries opened afterwards.
    pub fn with_condition_eq(&self, col: impl Into<String>, value: impl Into<CborValue>) -> Self {
        self.inner
            .base_conditions
            .write()
            .unwrap()
            .push((col.into(), value.into()));
        self.clone()
    }

    /// Set the Dio's default order, inherited by every scenery that doesn't set
    /// its own sort. Like [`with_condition_eq`](Self::with_condition_eq), the
    /// Dio owns ordering — native+orderable columns push down, others sort
    /// locally over the cache. Returns a clone so calls chain.
    pub fn with_order(&self, col: impl Into<String>, dir: crate::scenery::SortDir) -> Self {
        *self.inner.base_sort.write().unwrap() = Some((col.into(), dir));
        self.clone()
    }

    /// Configure two-pass augmentation on this Dio: a cheap master list pass plus
    /// a per-row detail pass that resolves each [`Augmentation`](crate::Augmentation)'s detail Vista
    /// through `catalog`, fetches it, and merges its columns onto the row.
    ///
    /// Augmentation is a property of the Dio, not the Lens — so different Dios
    /// sharing one Lens can enrich differently. The merged columns are recorded
    /// as the Dio's *augmented columns*; a condition or sort on one of them is
    /// client-side and routes to local emulation rather than master pushdown.
    ///
    /// Call before opening sceneries. Returns a clone so calls chain.
    pub fn augment(
        &self,
        catalog: Arc<vantage_vista_factory::VistaCatalog>,
        augmentations: Vec<crate::augment::Augmentation>,
    ) -> Self {
        let mut cols = std::collections::HashSet::new();
        for aug in &augmentations {
            for c in &aug.merge.columns {
                cols.insert(c.clone());
            }
        }
        *self.inner.augmented_columns.write().unwrap() = cols;
        *self.inner.augment_catalog.write().unwrap() = Some(catalog);
        *self.inner.augmentations.write().unwrap() = Some(Arc::new(augmentations));
        self.inner.ensure_augment_workers();
        self.clone()
    }

    /// Start a [`TableScenery`] builder
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
    /// v1). Use [`Dio::patched`](Self::patched) to seed the row.
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
        let name = self.master().name().to_string();
        let shell = DioShell::new(self.inner.clone());
        Vista::new(name, Box::new(shell))
    }

    /// Fetch a `[offset, limit)` window, preferring the master's own ordering.
    ///
    /// When `sort` is set and the master `can_order` and yields an independent
    /// [`clone_shell`](vantage_vista::TableShell::clone_shell), the window is read
    /// from a **per-call ordered clone** (`add_order` → `fetch_window`) — the
    /// shared master is never mutated, so differently-sorted views never race. If
    /// the master can't order or can't be cloned, it fetches the window in native
    /// order and the caller re-sorts over the cache (the existing fallback).
    pub async fn fetch_window_ordered(
        &self,
        offset: usize,
        limit: usize,
        sort: Option<(String, crate::SortDir)>,
    ) -> Result<Vec<(String, Record<CborValue>)>> {
        let master = self.master();
        if let Some((col, dir)) = sort
            && master.capabilities().can_order
            && let Some(shell) = master.source.clone_shell()
        {
            let mut ordered = Vista::new(master.name(), shell);
            let vdir = match dir {
                crate::SortDir::Asc => vantage_vista::SortDirection::Ascending,
                crate::SortDir::Desc => vantage_vista::SortDirection::Descending,
            };
            ordered.add_order(&col, vdir)?;
            return ordered.fetch_window(offset, limit).await;
        }
        master.fetch_window(offset, limit).await
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
    /// [`patched`](Self::patched) or [`notify_record_changed`](Self::notify_record_changed)).
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
    pub fn notify_record_changed(&self, id: impl Into<String>) {
        let _ = self
            .inner
            .event_bus
            .send(DioEvent::RecordChanged { id: id.into() });
    }

    /// Publish [`DioEvent::DatasetChanged`] on the bus — "the set of records
    /// changed: rows appeared, vanished, or reordered." Sceneries respond by
    /// re-deriving their index and re-reading their full state.
    pub fn notify_dataset_changed(&self) {
        let _ = self.inner.event_bus.send(DioEvent::DatasetChanged);
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
    /// master until the next `refresh()` / `notify_dataset_changed()`.
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
        let result = if self.inner.has_dio_augment() {
            // Dio owns augmentation → run its reconciling refresh pass.
            augment_passes::refresh(self).await
        } else if let Some(cb) = self.inner.lens.callbacks.on_refresh.as_ref() {
            cb(self).await
        } else {
            Ok(())
        };
        if result.is_ok() {
            let _ = self.inner.event_bus.send(DioEvent::DatasetChanged);
        }
        result
    }
}
