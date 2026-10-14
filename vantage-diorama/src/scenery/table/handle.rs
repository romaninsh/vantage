//! The live handle [`TableSceneryBuilder::open`](super::TableSceneryBuilder::open)
//! returns: the [`TableScenery`] implementation over one owned view.

mod reads;

use std::ops::Range;
use std::sync::Arc;

use futures::future::BoxFuture;
use tokio::sync::watch;
use vantage_core::Result;
use vantage_vista::{Condition, SortDirection, VistaCapabilities};

use crate::dio::Generation;
use crate::scenery::enriched_record::EnrichedRecord;

use super::state::SharedRows;
use super::view::{Placing, TableView};
use super::{
    LoadState, RowStatusSummary, TableScenery, ViewStats, ViewportRequest, count, loader, two_pass,
};

pub(crate) use reads::compute_load_state;
use reads::{cap_loaded, shared_estimate, shared_row};

/// Holds the caller's view. Dropping the last handle drops the view, which
/// detaches it from its rows. When it was their last view, their work — the
/// reactor, the viewport loop with any load in flight, a detached list pass —
/// is aborted at once, so a closing grid stops pulling.
pub(crate) struct TableSceneryImpl {
    pub(crate) view: Arc<TableView>,
}

impl TableSceneryImpl {
    /// How the view reads its rows now: `None` while refused, when it shows
    /// nothing whatever its rows hold.
    fn placing(&self) -> Option<(Arc<SharedRows>, Placing)> {
        if self.view.refusal().is_some() {
            return None;
        }
        let shared = self.view.shared();
        let placing = self.view.placing(&shared);
        Some((shared, placing))
    }
}

impl TableScenery for TableSceneryImpl {
    /// A capped view whose shown rows have all loaded is complete, even
    /// while its rows hold a larger set only partly.
    fn load_state(&self) -> LoadState {
        if let Some(failure) = self.view.refusal() {
            return LoadState::Failed(failure);
        }
        let shared = self.view.shared();
        let state = compute_load_state(&shared);
        match self.view.placing(&shared) {
            Placing::Unplaced(_) if !matches!(state, LoadState::Failed(_)) => LoadState::Loading,
            Placing::Capped(cap, len)
                if state == LoadState::Partial && cap_loaded(&shared, cap, len) =>
            {
                LoadState::Complete
            }
            _ => state,
        }
    }

    fn row_count(&self) -> usize {
        match self.placing() {
            None | Some((_, Placing::Unplaced(_))) => 0,
            Some((shared, Placing::Whole)) => shared.row_count(),
            Some((_, Placing::Capped(cap, len))) => cap.view_len(len),
        }
    }

    fn status_summary(&self) -> RowStatusSummary {
        match self.placing() {
            None | Some((_, Placing::Unplaced(_))) => RowStatusSummary::default(),
            Some((shared, _)) => shared.status_summary(),
        }
    }

    /// A capped view is complete once its cap is reached.
    fn has_more(&self) -> bool {
        match self.placing() {
            None | Some((_, Placing::Unplaced(_))) => false,
            Some((shared, Placing::Whole)) => shared.has_more(),
            Some((shared, Placing::Capped(cap, len))) => !cap.reached(len) && shared.has_more(),
        }
    }

    /// Agrees with [`view_stats`](Self::view_stats): `Some(0)` while
    /// refused, `None` while a tail waits for the end of its set.
    fn estimated_total(&self) -> Option<usize> {
        match self.placing() {
            None => Some(0),
            Some((_, Placing::Unplaced(_))) => None,
            Some((shared, Placing::Whole)) => shared_estimate(&shared),
            Some((shared, Placing::Capped(cap, _))) => {
                shared_estimate(&shared).map(|t| cap.view_len(t))
            }
        }
    }

    fn row(&self, idx: usize) -> Option<Arc<EnrichedRecord>> {
        let (shared, idx) = match self.placing()? {
            (shared, Placing::Whole) => (shared, idx),
            (shared, Placing::Capped(cap, len)) => {
                let idx = cap.map(idx, len)?;
                (shared, idx)
            }
            (_, Placing::Unplaced(_)) => return None,
        };
        shared_row(&shared, idx)
    }

    fn row_by_id(&self, id: &str) -> Option<Arc<EnrichedRecord>> {
        let (shared, placing) = self.placing()?;
        let idx = *shared.id_to_idx.read().unwrap().get(id)?;
        match placing {
            Placing::Whole => {}
            Placing::Capped(cap, len) => {
                cap.view_index(idx, len)?;
            }
            Placing::Unplaced(_) => return None,
        }
        shared_row(&shared, idx)
    }

    fn set_viewport(&self, loaded: Range<usize>, shown: Range<usize>) {
        self.view.set_viewport(loaded, shown);
    }

    /// A tail already asks for its whole cap; a limit stops at the cap.
    fn request_load_more(&self) {
        match self.placing() {
            None => return,
            Some((_, Placing::Unplaced(_))) => return,
            Some((_, Placing::Capped(cap, len))) if cap.is_tail() || cap.reached(len) => return,
            Some(_) => {}
        }
        let shared = self.view.shared();
        // Two-pass: load the next *list* page (append cheap rows to the index).
        // Detail hydration is driven separately by `set_viewport`.
        if shared.two_pass {
            let Some(dio_inner) = shared.dio_weak.upgrade() else {
                return;
            };
            let list = dio_inner
                .lens
                .runtime
                .spawn(two_pass::run_list_page(shared.clone()));
            shared.tasks.hold(list);
            return;
        }

        let start = shared.next_load_more_start();
        let mut end = start + self.view.page_size;
        if let Some(t) = *shared.total.read().unwrap() {
            end = end.min(t);
        }
        if end <= start {
            return;
        }
        loader::enqueue_viewport(
            &shared,
            ViewportRequest {
                range: start..end,
                force_load: true,
                priority: vantage_core::Priority::Essential,
                view: Some(self.view.id),
            },
        );
    }

    fn refresh_source(&self) {
        let Some(dio_inner) = self.view.dio_weak.upgrade() else {
            return;
        };
        let runtime = dio_inner.lens.runtime.clone();
        runtime.spawn(async move {
            let dio = crate::Dio { inner: dio_inner };
            if let Err(e) = dio.refresh().await {
                tracing::error!(error = %e, "Scenery refresh_source failed");
            }
        });
    }

    fn set_order(&self, order: Option<(String, SortDirection)>) {
        self.view.set_order(order);
    }

    fn set_filters(&self, filters: Vec<Condition>) {
        self.view.set_filters(filters);
    }

    fn count_where(&self, filters: Vec<Condition>) -> BoxFuture<'static, Result<Option<usize>>> {
        count::count_where(&self.view, filters)
    }

    fn filters_supported(&self, filters: &[Condition]) -> bool {
        count::filters_supported(&self.view, filters)
    }

    fn set_search(&self, query: Option<String>) {
        self.view.set_search(query);
    }

    fn search_supported(&self) -> bool {
        count::search_supported(&self.view)
    }

    fn subscribe(&self) -> watch::Receiver<Generation> {
        self.view.signal.subscribe()
    }

    fn view_stats(&self) -> ViewStats {
        self.view.view_stats()
    }

    fn subscribe_view_stats(&self) -> watch::Receiver<Generation> {
        self.view.signal.subscribe_stats()
    }

    fn master_capabilities(&self) -> &VistaCapabilities {
        &self.view.master_capabilities
    }
}
