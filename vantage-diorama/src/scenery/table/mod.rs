//! `TableScenery` — reactive ordered-rows view onto a Dio.
//!
//! v2 implementation. The Scenery holds a sparse
//! `BTreeMap<usize, Arc<EnrichedRecord>>` keyed by row index. Rows
//! arrive via two paths:
//!
//! - **Cache seed** — `open()` reads whatever is already in the cache
//!   (e.g. warmed from disk on restart) and assigns indices in
//!   iteration order. Subsequent `Invalidated` / `Refreshing` events
//!   re-seed the same way.
//! - **Chunk load** — `set_viewport` / `request_load_more` queue a
//!   range request on a debounce channel; on commit, the lens-level
//!   `on_load_chunk` callback fetches the missing indices from the
//!   master and streams them back through [`ChunkSink`](crate::ChunkSink).
//!
//! The reactor task ignores the scenery's own viewport events
//! (`ViewportChanged`, `RangeLoaded`, `LoadFailed`) to avoid looping
//! on its own output.

mod builder;
mod helpers;
mod loader;
mod reactor;
mod state;
mod two_pass;

use std::ops::Range;
use std::sync::Arc;

use tokio::sync::watch;
use vantage_vista::VistaCapabilities;

use crate::dio::Generation;

use super::enriched_record::EnrichedRecord;

pub use builder::TableSceneryBuilder;
pub(crate) use state::TableSceneryState;

/// UI-side sort direction. Mirrors `vantage_vista::SortDirection` but
/// kept distinct so Scenery callers don't need to import vista types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDir {
    Asc,
    Desc,
}

/// Internal viewport request carried over the debounce channel.
#[derive(Debug, Clone)]
pub(crate) struct ViewportRequest {
    pub(crate) range: Range<usize>,
    /// `request_load_more` sets this true so a fully-cached range
    /// still triggers a fetch (paging past the cache end).
    pub(crate) force_load: bool,
}

/// Breakdown of the row statuses currently materialized in a scenery's sparse
/// map. Cheap to compute (iterates only loaded rows, not the full row count) —
/// the per-scenery slice of the diagnostics surface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RowStatusSummary {
    /// Rows actually present in the sparse map (a paged scenery's `row_count`
    /// can be far larger — most indices are unloaded).
    pub loaded: usize,
    pub fresh: usize,
    pub incomplete: usize,
    pub pending_write: usize,
    /// `LoadFailed` + `WriteFailed` combined.
    pub failed: usize,
}

/// Reactive view onto a Dio that exposes an ordered, paginated row set.
pub trait TableScenery: Send + Sync {
    fn row_count(&self) -> usize;

    /// Status breakdown over the rows currently in the sparse map. Used by the
    /// diagnostics surface to report how much of a scenery is hydrated.
    fn status_summary(&self) -> RowStatusSummary;
    fn has_more(&self) -> bool;
    fn estimated_total(&self) -> Option<usize>;
    fn row(&self, idx: usize) -> Option<Arc<EnrichedRecord>>;

    fn set_viewport(&self, range: Range<usize>);
    fn request_load_more(&self);
    fn request_refresh(&self);
    fn set_search(&self, query: Option<String>);
    fn set_sort(&self, column: Option<String>, dir: SortDir);

    fn subscribe(&self) -> watch::Receiver<Generation>;

    /// Snapshot of the master Vista's capability flags taken at open
    /// time. UI delegates branch on these to pick the right page
    /// primitive: `can_fetch_page` → drive everything through
    /// `set_viewport`; cursor-only (`can_fetch_next`) → call
    /// `request_load_more` to walk forward.
    fn master_capabilities(&self) -> &VistaCapabilities;
}

pub(crate) struct TableSceneryImpl {
    pub(crate) inner: Arc<TableSceneryState>,
    /// Aborts the reactor + viewport tasks when the last handle to this
    /// scenery is released. The viewport task owns every in-flight fetch
    /// inline (single-pass chunk loads *and* two-pass detail hydration both
    /// run inside it), so aborting it cancels outstanding requests — a
    /// closing grid stops pulling. Dropping `inner` alone wouldn't suffice:
    /// the tasks hold their own `Arc<TableSceneryState>`, so without this
    /// guard a released scenery would linger for the Dio's whole lifetime.
    _guard: SceneryGuard,
}

/// Owns the scenery's background tasks and aborts them on drop. Deliberately
/// holds nothing else: registry eviction is lazy (`Weak::upgrade` + prune),
/// so a scenery that loses a concurrent-open race can drop safely without
/// touching the winner's registry entry.
struct SceneryGuard {
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for SceneryGuard {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl TableScenery for TableSceneryImpl {
    fn row_count(&self) -> usize {
        if let Some(index) = self.inner.index() {
            return index.len();
        }
        if let Some(t) = *self.inner.total.read().unwrap() {
            return t;
        }
        self.inner.rows.read().unwrap().len()
    }

    fn status_summary(&self) -> RowStatusSummary {
        use super::enriched_record::RowStatus;
        let mut s = RowStatusSummary::default();
        for row in self.inner.rows.read().unwrap().values() {
            s.loaded += 1;
            match &row.status {
                RowStatus::Fresh => s.fresh += 1,
                RowStatus::Incomplete => s.incomplete += 1,
                RowStatus::PendingWrite => s.pending_write += 1,
                RowStatus::LoadFailed { .. } | RowStatus::WriteFailed { .. } => s.failed += 1,
                _ => {}
            }
        }
        s
    }

    fn has_more(&self) -> bool {
        // Two-pass / sequential no-total: more pages exist until the list pass
        // sees a short or empty page.
        if let Some(index) = self.inner.index() {
            return !index.is_complete();
        }
        let total = *self.inner.total.read().unwrap();
        let loaded = self.inner.rows.read().unwrap().len();
        match total {
            Some(t) => loaded < t,
            None => false,
        }
    }

    fn estimated_total(&self) -> Option<usize> {
        // Two-pass: the running index length is the best estimate; it grows as
        // pages load and freezes once the list pass completes.
        if let Some(index) = self.inner.index() {
            return Some(index.len());
        }
        let stored = *self.inner.total.read().unwrap();
        stored.or_else(|| Some(self.inner.rows.read().unwrap().len()))
    }

    fn row(&self, idx: usize) -> Option<Arc<EnrichedRecord>> {
        if let Some(t) = *self.inner.total.read().unwrap()
            && idx >= t
        {
            return None;
        }
        self.inner.rows.read().unwrap().get(&idx).cloned()
    }

    fn set_viewport(&self, range: Range<usize>) {
        loader::enqueue_viewport(
            &self.inner,
            ViewportRequest {
                range,
                force_load: false,
            },
        );
    }

    fn request_load_more(&self) {
        // Two-pass: load the next *list* page (append cheap rows to the index).
        // Detail hydration is driven separately by `set_viewport`.
        if self.inner.two_pass {
            let Some(dio_inner) = self.inner.dio_weak.upgrade() else {
                return;
            };
            let state = self.inner.clone();
            dio_inner.lens.runtime.spawn(async move {
                two_pass::run_list_page(state).await;
            });
            return;
        }

        let start = self.inner.next_load_more_start();
        let mut end = start + self.inner.page_size;
        if let Some(t) = *self.inner.total.read().unwrap() {
            end = end.min(t);
        }
        if end <= start {
            return;
        }
        loader::enqueue_viewport(
            &self.inner,
            ViewportRequest {
                range: start..end,
                force_load: true,
            },
        );
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
        });
    }

    fn set_search(&self, query: Option<String>) {
        self.inner.deregister();
        *self.inner.search.write().unwrap() = query;
        self.inner.reload_notify.notify_one();
    }

    fn set_sort(&self, column: Option<String>, dir: SortDir) {
        self.inner.deregister();
        *self.inner.sort.write().unwrap() = column.map(|c| (c, dir));
        self.inner.reload_notify.notify_one();
    }

    fn subscribe(&self) -> watch::Receiver<Generation> {
        self.inner.generation_tx.subscribe()
    }

    fn master_capabilities(&self) -> &VistaCapabilities {
        &self.inner.master_capabilities
    }
}
