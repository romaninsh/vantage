//! `TableScenery` — reactive ordered-rows view onto a Dio.
//!
//! Stage 5 ships an eager, in-memory implementation: open the Scenery,
//! load every row matching the filter from cache, apply search + sort
//! in memory, expose the result through synchronous accessors. A
//! background task watches `dio.subscribe_events()` and reloads on any
//! event that could affect visible rows.
//!
//! Deliberately *not* in v1:
//!
//! - Sparse row vector / viewport prefetch — `set_viewport` and
//!   `request_load_more` are accepted as trait shape but no-op.
//! - Pagination — every matching row is loaded into the vector at
//!   open time.
//! - Hot tier (moka) sharing — `EnrichedRecord` is cloned per Scenery.
//! - Sort/search push-down to the cache — both are in-memory until
//!   vista stage 5b exposes `add_order` / `add_search` on Vista.

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use ciborium::Value as CborValue;
use tokio::sync::{Notify, broadcast, watch};
use vantage_core::Result;
use vantage_types::Record;

use crate::dio::{DioEvent, DioInner, Generation};

use super::enriched_record::EnrichedRecord;

/// UI-side sort direction. Mirrors `vantage_vista::SortDirection` but
/// kept distinct so Scenery callers don't need to import vista types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// Reactive view onto a Dio that exposes an ordered, paginated row set.
///
/// Trait surface is the full v2 shape (viewport, load-more, etc.) so
/// UI code can target it from day one. The v1 implementation honors
/// the load + react + sort + search bits; viewport/load-more are
/// accepted but no-op.
pub trait TableScenery: Send + Sync {
    fn row_count(&self) -> usize;
    fn has_more(&self) -> bool;
    fn estimated_total(&self) -> Option<usize>;
    fn row(&self, idx: usize) -> Option<Arc<EnrichedRecord>>;

    /// v1: no-op. Sparse vectors + prefetch land when there's a
    /// network-backed cache to justify them.
    fn set_viewport(&self, range: Range<usize>);
    /// v1: no-op. Everything matching the filter is already loaded.
    fn request_load_more(&self);
    fn request_refresh(&self);
    fn set_search(&self, query: Option<String>);
    fn set_sort(&self, column: Option<String>, dir: SortDir);

    fn subscribe(&self) -> watch::Receiver<Generation>;
}

/// Builder produced by [`Dio::table_scenery`](crate::Dio::table_scenery).
pub struct TableSceneryBuilder {
    pub(crate) dio: Arc<DioInner>,
    pub(crate) conditions: Vec<(String, CborValue)>,
    pub(crate) sort: Option<(String, SortDir)>,
    pub(crate) search: Option<String>,
    pub(crate) page_size: usize,
    pub(crate) eager: bool,
}

impl TableSceneryBuilder {
    pub(crate) fn new(dio: Arc<DioInner>) -> Self {
        Self {
            dio,
            conditions: Vec::new(),
            sort: None,
            search: None,
            page_size: 50,
            eager: false,
        }
    }

    pub fn where_eq(mut self, col: impl Into<String>, value: impl Into<CborValue>) -> Self {
        self.conditions.push((col.into(), value.into()));
        self
    }

    pub fn sort(mut self, col: impl Into<String>, dir: SortDir) -> Self {
        self.sort = Some((col.into(), dir));
        self
    }

    pub fn search(mut self, q: impl Into<String>) -> Self {
        self.search = Some(q.into());
        self
    }

    /// v1: stored but ignored (everything is loaded). Kept so caller
    /// code targets the eventual API shape.
    pub fn page_size(mut self, n: usize) -> Self {
        self.page_size = n;
        self
    }

    /// v1: redundant — every Scenery is effectively eager. Kept so
    /// caller code targets the eventual API shape.
    pub fn eager(mut self) -> Self {
        self.eager = true;
        self
    }

    /// Open the Scenery. Spawns the per-Scenery reload task, performs
    /// the initial load, and returns the live handle.
    pub async fn open(self) -> Result<Arc<dyn TableScenery>> {
        let TableSceneryBuilder {
            dio,
            conditions,
            sort,
            search,
            page_size: _,
            eager: _,
        } = self;

        let (gen_tx, _gen_rx) = watch::channel(Generation::default());
        let inner = Arc::new(TableSceneryState {
            dio_weak: Arc::downgrade(&dio),
            conditions: RwLock::new(conditions),
            sort: RwLock::new(sort),
            search: RwLock::new(search),
            rows: RwLock::new(Vec::new()),
            generation: AtomicU64::new(0),
            generation_tx: gen_tx,
            reload_notify: Arc::new(Notify::new()),
        });

        // Initial load — block on it so callers see populated rows
        // immediately after `.open()` returns.
        inner.reload().await?;

        // Spawn the background reactor.
        let bus_rx = dio.event_bus.subscribe();
        let task_inner = inner.clone();
        dio.lens.runtime.spawn(async move {
            reload_loop(task_inner, bus_rx).await;
        });

        Ok(Arc::new(TableSceneryImpl { inner }) as Arc<dyn TableScenery>)
    }
}

pub(crate) struct TableSceneryState {
    /// Weak so the Scenery doesn't pin the Dio alive — task drops out
    /// when the last user-held Dio drops.
    pub(crate) dio_weak: std::sync::Weak<DioInner>,

    // Filter / sort / search — mutable through setters on the Scenery.
    pub(crate) conditions: RwLock<Vec<(String, CborValue)>>,
    pub(crate) sort: RwLock<Option<(String, SortDir)>>,
    pub(crate) search: RwLock<Option<String>>,

    // Loaded rows.
    pub(crate) rows: RwLock<Vec<Arc<EnrichedRecord>>>,

    // Reactivity.
    pub(crate) generation: AtomicU64,
    pub(crate) generation_tx: watch::Sender<Generation>,
    pub(crate) reload_notify: Arc<Notify>,
}

impl TableSceneryState {
    async fn reload(&self) -> Result<()> {
        let Some(dio_inner) = self.dio_weak.upgrade() else {
            return Ok(());
        };

        // Read everything from cache. Conditions/search/sort apply in
        // memory until vista 5b lands push-down through DioShell.
        let all = dio_inner.cache.list_values().await?;

        let conditions = self.conditions.read().unwrap().clone();
        let search = self.search.read().unwrap().clone();
        let sort = self.sort.read().unwrap().clone();

        let mut filtered: Vec<(String, Record<CborValue>)> = all
            .into_iter()
            .filter(|(_, rec)| matches_conditions(rec, &conditions))
            .filter(|(_, rec)| matches_search(rec, search.as_deref()))
            .collect();

        if let Some((col, dir)) = sort {
            filtered.sort_by(|(_, a), (_, b)| {
                let ord = cbor_cmp(a.get(&col), b.get(&col));
                match dir {
                    SortDir::Asc => ord,
                    SortDir::Desc => ord.reverse(),
                }
            });
        }

        let enriched: Vec<Arc<EnrichedRecord>> = filtered
            .into_iter()
            .map(|(_, rec)| Arc::new(EnrichedRecord::fresh(rec)))
            .collect();

        *self.rows.write().unwrap() = enriched;
        self.bump_generation();
        Ok(())
    }

    fn bump_generation(&self) {
        let next = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = self.generation_tx.send(Generation(next));
    }

    fn schedule_reload(&self) {
        self.reload_notify.notify_one();
    }
}

async fn reload_loop(state: Arc<TableSceneryState>, mut bus: broadcast::Receiver<DioEvent>) {
    loop {
        // Exit when the last external Dio drops — Weak upgrade fails.
        if state.dio_weak.upgrade().is_none() {
            return;
        }

        tokio::select! {
            _ = state.reload_notify.notified() => {
                if let Err(e) = state.reload().await {
                    tracing::error!(error = %e, "TableScenery reload failed");
                }
            }
            recv = bus.recv() => {
                match recv {
                    Ok(DioEvent::RecordChanged { .. })
                    | Ok(DioEvent::RecordInserted { .. })
                    | Ok(DioEvent::RecordRemoved { .. })
                    | Ok(DioEvent::Invalidated)
                    | Ok(DioEvent::Refreshing) => {
                        if let Err(e) = state.reload().await {
                            tracing::error!(error = %e, "TableScenery reload failed");
                        }
                    }
                    Ok(DioEvent::WriteFailed { .. }) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // We missed some events; safest is a full reload.
                        if let Err(e) = state.reload().await {
                            tracing::error!(error = %e, "TableScenery reload failed");
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
        }
    }
}

pub(crate) struct TableSceneryImpl {
    pub(crate) inner: Arc<TableSceneryState>,
}

impl TableScenery for TableSceneryImpl {
    fn row_count(&self) -> usize {
        self.inner.rows.read().unwrap().len()
    }

    fn has_more(&self) -> bool {
        false
    }

    fn estimated_total(&self) -> Option<usize> {
        Some(self.row_count())
    }

    fn row(&self, idx: usize) -> Option<Arc<EnrichedRecord>> {
        self.inner.rows.read().unwrap().get(idx).cloned()
    }

    fn set_viewport(&self, _range: Range<usize>) {
        // v1: no-op. See module-level doc.
    }

    fn request_load_more(&self) {
        // v1: no-op. See module-level doc.
    }

    fn request_refresh(&self) {
        let Some(dio_inner) = self.inner.dio_weak.upgrade() else {
            return;
        };
        let runtime = dio_inner.lens.runtime.clone();
        runtime.spawn(async move {
            let dio = crate::Dio { inner: dio_inner };
            if let Err(e) = dio.refresh().await {
                tracing::error!(error = %e, "Scenery request_refresh failed");
            }
            // refresh() publishes `Invalidated`; the reload loop picks it up.
        });
    }

    fn set_search(&self, query: Option<String>) {
        *self.inner.search.write().unwrap() = query;
        self.inner.schedule_reload();
    }

    fn set_sort(&self, column: Option<String>, dir: SortDir) {
        *self.inner.sort.write().unwrap() = column.map(|c| (c, dir));
        self.inner.schedule_reload();
    }

    fn subscribe(&self) -> watch::Receiver<Generation> {
        self.inner.generation_tx.subscribe()
    }
}

// ---- helpers ---------------------------------------------------------------

fn matches_conditions(rec: &Record<CborValue>, conds: &[(String, CborValue)]) -> bool {
    conds.iter().all(|(col, expected)| match rec.get(col) {
        Some(v) => cbor_eq(v, expected),
        None => false,
    })
}

fn matches_search(rec: &Record<CborValue>, needle: Option<&str>) -> bool {
    let Some(needle) = needle else {
        return true;
    };
    let needle_lc = needle.to_lowercase();
    rec.values().any(|v| match v {
        CborValue::Text(s) => s.to_lowercase().contains(&needle_lc),
        _ => false,
    })
}

fn cbor_eq(a: &CborValue, b: &CborValue) -> bool {
    match (a, b) {
        (CborValue::Text(x), CborValue::Text(y)) => x == y,
        (CborValue::Integer(x), CborValue::Integer(y)) => x == y,
        (CborValue::Bool(x), CborValue::Bool(y)) => x == y,
        // Float and the rest fall back to format-string compare. Good
        // enough for v1's hand-rolled filter.
        _ => format!("{a:?}") == format!("{b:?}"),
    }
}

fn cbor_cmp(a: Option<&CborValue>, b: Option<&CborValue>) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, _) => Ordering::Less,
        (_, None) => Ordering::Greater,
        (Some(lhs), Some(rhs)) => match (lhs, rhs) {
            (CborValue::Text(l), CborValue::Text(r)) => l.cmp(r),
            (CborValue::Integer(l), CborValue::Integer(r)) => i128::from(*l).cmp(&i128::from(*r)),
            (CborValue::Bool(l), CborValue::Bool(r)) => l.cmp(r),
            _ => format!("{lhs:?}").cmp(&format!("{rhs:?}")),
        },
    }
}
