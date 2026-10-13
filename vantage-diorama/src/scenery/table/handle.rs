//! The live handle [`TableSceneryBuilder::open`](super::TableSceneryBuilder::open)
//! returns: the [`TableScenery`] implementation over the shared state.

use std::ops::Range;
use std::sync::Arc;

use tokio::sync::watch;
use vantage_vista::{Capability, VistaCapabilities};

use crate::dio::{DioInner, Generation};
use crate::scenery::enriched_record::EnrichedRecord;

use super::state::TableSceneryState;
use super::{
    LoadState, OpCondition, RowStatusSummary, SortDir, TableScenery, ViewStats, ViewportRequest,
    loader, two_pass,
};

pub(crate) struct TableSceneryImpl {
    pub(crate) inner: Arc<TableSceneryState>,
    /// Aborts the reactor + viewport tasks when the last handle to this
    /// scenery is released. The viewport task owns every in-flight fetch
    /// inline (single-pass chunk loads *and* two-pass detail hydration both
    /// run inside it), so aborting it cancels outstanding requests — a
    /// closing grid stops pulling. Dropping `inner` alone wouldn't suffice:
    /// the tasks hold their own `Arc<TableSceneryState>`, so without this
    /// guard a released scenery would linger for the Dio's whole lifetime.
    pub(super) _guard: SceneryGuard,
}

/// Owns the scenery's background tasks and aborts them on drop. Deliberately
/// holds nothing else: registry eviction is lazy (`Weak::upgrade` + prune),
/// so a scenery that loses a concurrent-open race can drop safely without
/// touching the winner's registry entry.
pub(super) struct SceneryGuard {
    pub(super) tasks: Vec<tokio::task::JoinHandle<()>>,
    /// Weak — the guard must not keep the Dio alive; it only reaches back
    /// in to emit the closing census line, and only if the Dio is still
    /// there to receive it.
    pub(super) dio_weak: std::sync::Weak<DioInner>,
}

impl Drop for SceneryGuard {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        if let Some(dio) = self.dio_weak.upgrade() {
            dio.emit_census("table scenery", "closed");
        }
    }
}

/// The [`LoadState`] computation, shared by [`TableScenery::load_state`] and
/// [`TableSceneryState::note_state`] so the trait method and the debug stream
/// can never disagree about what state a scenery is in.
pub(crate) fn compute_load_state(state: &TableSceneryState) -> LoadState {
    if let super::plan::Serving::Failed(failure) = state.serving() {
        return LoadState::Failed(failure);
    }
    if !state.settled.load(std::sync::atomic::Ordering::SeqCst) {
        return LoadState::Loading;
    }
    if state
        .omits_failed_rows
        .load(std::sync::atomic::Ordering::SeqCst)
    {
        return LoadState::Partial;
    }
    // A known total the visible map has not reached means rows are still
    // coming — the ordinary state of a paged grid before it is scrolled.
    // With no total there is nothing to be short of, so what is loaded is
    // all there is.
    let loaded = state.rows.read().unwrap().len();
    match *state.total.read().unwrap() {
        Some(total) if loaded < total => LoadState::Partial,
        _ => LoadState::Complete,
    }
}

impl TableScenery for TableSceneryImpl {
    fn load_state(&self) -> LoadState {
        compute_load_state(&self.inner)
    }

    fn row_count(&self) -> usize {
        self.inner.row_count()
    }

    fn status_summary(&self) -> RowStatusSummary {
        self.inner.status_summary()
    }

    fn has_more(&self) -> bool {
        self.inner.has_more()
    }

    fn estimated_total(&self) -> Option<usize> {
        if self.inner.local_refine() {
            return Some(self.inner.rows.read().unwrap().len());
        }
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

    fn set_shown_range(&self, range: Option<Range<usize>>) {
        self.inner.set_shown_range(range);
    }

    fn set_viewport(&self, range: Range<usize>) {
        loader::enqueue_viewport(
            &self.inner,
            ViewportRequest {
                range,
                force_load: false,
                priority: vantage_core::Priority::Essential,
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
                priority: vantage_core::Priority::Essential,
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

    fn set_sort(&self, column: Option<String>, dir: SortDir) {
        self.inner.set_sort(column, dir);
    }

    fn set_filters(&self, filters: Vec<(String, ciborium::Value)>) {
        self.inner.set_filters(filters);
    }

    fn set_filter_terms(&self, terms: Vec<OpCondition>) {
        self.inner.set_filter_terms(terms);
    }

    fn filter_terms(&self) -> Vec<OpCondition> {
        self.inner.ui_terms.read().unwrap().clone()
    }

    fn set_search(&self, query: Option<String>) {
        self.inner.set_search(query);
    }

    fn search_supported(&self) -> bool {
        if self.inner.paged {
            self.inner.master_capabilities.has(Capability::Search)
        } else {
            true
        }
    }

    fn subscribe(&self) -> watch::Receiver<Generation> {
        self.inner.generation_tx.subscribe()
    }

    fn view_stats(&self) -> ViewStats {
        self.inner.stats.snapshot()
    }

    fn subscribe_view_stats(&self) -> watch::Receiver<Generation> {
        self.inner.stats.subscribe()
    }

    fn master_capabilities(&self) -> &VistaCapabilities {
        &self.inner.master_capabilities
    }

    fn demanded_columns(&self) -> Option<Vec<String>> {
        self.inner.demand.clone()
    }
}
