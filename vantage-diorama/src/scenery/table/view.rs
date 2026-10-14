//! A table view: one owner's query, viewport and shown range, over the
//! [`SharedRows`] that answer that query.
//!
//! Every `open()` builds a view of its own. Views of one query read the same
//! `SharedRows`, so the rows are fetched once however many views show them;
//! a view's setters change only its own query and move it to the rows of
//! the new one (see `repoint`).

mod repoint;
mod setters;
mod signal;
mod tail;

use std::ops::Range;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};

use vantage_vista::{Condition, SortDirection, VistaCapabilities, VistaQuery};

use crate::dio::DioInner;
use crate::dio::augment_scheduler::AugmentTicket;

use super::cap::Cap;
use super::state::SharedRows;
use super::view_stats::{clamp_range, view_filters};
use super::{
    LoadFailure, LoadState, ViewState, ViewStats, ViewportRequest, compute_load_state, loader,
};

pub(crate) use signal::{ViewHook, ViewSignal};
pub(crate) use tail::{Placing, follow_tail};

/// Identifies one open view to the rows it reads.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct ViewId(u64);

impl ViewId {
    pub(crate) fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }
}

pub(crate) struct TableView {
    /// Live-instance census (see [`crate::stats`]).
    pub(crate) _tally: crate::stats::Tally,
    pub(crate) id: ViewId,
    pub(crate) dio_weak: Weak<DioInner>,
    /// The builder's [`filter`](super::TableSceneryBuilder::filter)s, fixed
    /// at open.
    pub(crate) base: Vec<Condition>,
    /// The runtime filters ([`set_filters`](super::TableScenery::set_filters)),
    /// ANDed with `base`. Kept apart from it so replacing them never
    /// clobbers what the view was opened with.
    pub(crate) filters: RwLock<Vec<Condition>>,
    /// Active quicksearch text (`None` when not searching).
    pub(crate) search: RwLock<Option<String>>,
    /// The declared order. A flipped tail's rows answer it flipped.
    pub(crate) order: RwLock<Option<(String, SortDirection)>>,
    /// The builder's [`limit`](super::TableSceneryBuilder::limit) or
    /// [`tail`](super::TableSceneryBuilder::tail). A tail over windows that
    /// never state a total turns into a flipped one (see `tail`); otherwise
    /// fixed at open. Written under `repointing`.
    pub(crate) cap: RwLock<Option<Cap>>,
    /// The set's end a tail last asked its rows for, so it asks again only
    /// when the end moves.
    pub(crate) placed_end: Mutex<Option<usize>>,
    /// Why the view shows no rows whatever its rows hold: a tail whose
    /// order was cleared. Cleared when an order is set again. Written under
    /// `repointing`.
    pub(crate) refused: RwLock<Option<LoadFailure>>,
    /// The rows answering the view's current query. Swapped by a setter;
    /// held strongly, so the rows live while a view points at them.
    pub(crate) shared: RwLock<Arc<SharedRows>>,
    /// The rows a displaying consumer last declared on screen, the second
    /// argument of `set_viewport`, in the view's own indices. `None` when it
    /// shows none: a dashboard counter or an observation probe loads rows
    /// without showing any of them.
    pub(crate) shown: RwLock<Option<Range<usize>>>,
    /// Hint range used by `request_load_more`, and the page size of any rows
    /// this view builds.
    pub(crate) page_size: usize,
    pub(crate) titles_only: bool,
    /// The columns this view declared it shows (its **demand**), from the
    /// builder's `columns()`. `None` = demands everything. The Dio unions the
    /// demands of its open views to gate the augment detail pass — see
    /// [`DioInner::demanded_columns`](crate::dio::DioInner::demanded_columns).
    pub(crate) demand: Option<Vec<String>>,
    /// Whether the view reads through sugar. Its rows are its own: they are
    /// never looked up or registered, so no other view shares them.
    pub(crate) sugared: bool,
    /// This view's requester handle into the Dio's augment scheduler
    /// (two-pass only), so two views keep their own round-robin places.
    /// Dropping the last handle withdraws what it still has queued.
    pub(crate) ticket: Option<Arc<AugmentTicket>>,
    pub(crate) signal: Arc<ViewSignal>,
    pub(crate) master_capabilities: VistaCapabilities,
    /// Held across a re-point, so two setters racing on one view move it
    /// once each, in turn.
    pub(crate) repointing: Mutex<()>,
}

impl TableView {
    /// The rows the view reads now.
    pub(crate) fn shared(&self) -> Arc<SharedRows> {
        self.shared.read().unwrap().clone()
    }

    /// A hook for attaching this view to rows.
    pub(crate) fn hook(&self) -> ViewHook {
        ViewHook::new(&self.signal, self.ticket.clone())
    }

    pub(crate) fn cap(&self) -> Option<Cap> {
        *self.cap.read().unwrap()
    }

    /// Every clause of this view as one query: the builder's filters, the
    /// runtime filters, search and the order its rows load in.
    pub(crate) fn view_query(&self) -> VistaQuery {
        self.query_with_filters(&self.filters.read().unwrap())
    }

    /// [`view_query`](Self::view_query) with `filters` standing in for the
    /// view's runtime filters.
    pub(crate) fn query_with_filters(&self, filters: &[Condition]) -> VistaQuery {
        let order = self.order.read().unwrap().clone();
        compose(
            &self.base,
            filters,
            self.search.read().unwrap().clone(),
            self.cap()
                .map_or(order.clone(), |cap| cap.load_order(order)),
        )
    }

    /// The columns this view declared it shows (its demand); `None` demands
    /// everything.
    pub(crate) fn demanded_columns(&self) -> Option<Vec<String>> {
        self.demand.clone()
    }

    /// Ask the rows for `loaded` (in the view's indices, mapped through its
    /// cap), and record `shown` as the rows on screen; empty means none.
    /// The load is serialised with re-points, so a viewport is never
    /// recorded into rows the view has just left.
    pub(crate) fn set_viewport(&self, loaded: Range<usize>, shown: Range<usize>) {
        {
            let _serial = self.repointing.lock().unwrap_or_else(|e| e.into_inner());
            let shared = self.shared();
            let range = match self.placing(&shared) {
                Placing::Whole => loaded,
                Placing::Capped(cap, len) => cap.shared_range(loaded, len, shared.two_pass),
                Placing::Unplaced(cap) => cap.shared_range(loaded, 0, shared.two_pass),
            };
            self.ask(&shared, range);
        }
        let shown = self.cap().map_or(shown.clone(), |cap| cap.clamp(shown));
        let shown = Some(shown).filter(|r| !r.is_empty());
        let previous = std::mem::replace(&mut *self.shown.write().unwrap(), shown.clone());
        if previous != shown {
            self.signal.bump_stats();
        }
    }

    /// Record `range` as the shared rows this view asks for, and ask for
    /// them. The caller holds `repointing`.
    fn ask(&self, shared: &SharedRows, range: Range<usize>) {
        shared.record_viewport(self.id, range.clone());
        loader::enqueue_viewport(
            shared,
            ViewportRequest {
                range,
                force_load: false,
                priority: vantage_core::Priority::Essential,
                view: Some(self.id),
            },
        );
    }

    /// The view's refusal, when it shows no rows whatever its rows hold.
    pub(crate) fn refusal(&self) -> Option<LoadFailure> {
        self.refused.read().unwrap().clone()
    }

    /// The rows' snapshot with this view's shown range (clamped to the rows
    /// it has), its declared order, its own filters and its cap laid over it.
    pub(crate) fn view_stats(&self) -> ViewStats {
        let shared = self.shared();
        let mut stats = shared.stats.snapshot();
        stats.showing = clamp_range(self.shown.read().unwrap().clone(), shared.row_count());
        stats.order = self.order.read().unwrap().clone();
        stats.filters = view_filters(&self.query_filters(), &self.filters.read().unwrap());
        let nothing = ViewStats {
            loaded: 0,
            showing: None,
            has_more: false,
            ..stats.clone()
        };
        if self.refusal().is_some() {
            return ViewStats {
                total: Some(0),
                total_exact: true,
                state: ViewState::Failed,
                ..nothing
            };
        }
        match self.placing(&shared) {
            Placing::Whole => stats,
            Placing::Capped(cap, _) => stats.capped(cap.n()),
            // As `load_state`: loading, unless the rows failed their query.
            Placing::Unplaced(_) => ViewStats {
                total: None,
                total_exact: false,
                state: match compute_load_state(&shared) {
                    LoadState::Failed(_) => ViewState::Failed,
                    _ => ViewState::Loading,
                },
                ..nothing
            },
        }
    }

    /// The conditions that are part of what the view is: the Dio's residual
    /// ones, then the builder's.
    fn query_filters(&self) -> Vec<Condition> {
        let residual = self
            .dio_weak
            .upgrade()
            .map(|dio| dio.residual.conditions.clone())
            .unwrap_or_default();
        residual
            .into_iter()
            .chain(self.base.iter().cloned())
            .collect()
    }
}

impl Drop for TableView {
    /// Detach from the rows: they stop signalling this view and forget its
    /// viewport. When it was the last view on them, their work is aborted
    /// at once and they drop with it.
    fn drop(&mut self) {
        let shared = self
            .shared
            .get_mut()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        shared.detach(self.id);
        if let Some(dio) = self.dio_weak.upgrade() {
            dio.emit_census("table scenery", "closed");
        }
    }
}

/// One query from a view's clauses.
pub(crate) fn compose(
    base: &[Condition],
    filters: &[Condition],
    search: Option<String>,
    order: Option<(String, SortDirection)>,
) -> VistaQuery {
    let mut query = VistaQuery::new();
    query.conditions = base.iter().chain(filters).cloned().collect();
    query.search = search;
    query.order = order;
    query
}
