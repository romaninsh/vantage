//! A row-cap decorator over a [`TableScenery`].
//!
//! The view a UI-level `limit:` produces: every consumer of the trait —
//! hosted grids, observation adapters, scrollbar sizing — sees at most
//! `cap` rows through one wrapper, instead of each consumer re-implementing
//! the truncation. Capping the *scenery* (not the hydration viewport) also
//! means a master without windowed loading is never asked to serve a
//! viewport contract it can't: the underlying scenery keeps its own loading
//! mode (eager cache seed or chunked), and the cap only bounds what is
//! visible.

use std::ops::Range;
use std::sync::Arc;

use tokio::sync::watch;
use vantage_vista::VistaCapabilities;

use crate::dio::Generation;
use crate::scenery::enriched_record::EnrichedRecord;

use super::{RowStatusSummary, SortDir, TableScenery};

pub struct CappedScenery {
    inner: Arc<dyn TableScenery>,
    cap: usize,
}

impl CappedScenery {
    pub fn wrap(inner: Arc<dyn TableScenery>, cap: usize) -> Arc<Self> {
        Arc::new(Self { inner, cap })
    }
}

impl TableScenery for CappedScenery {
    fn row_count(&self) -> usize {
        self.inner.row_count().min(self.cap)
    }

    fn status_summary(&self) -> RowStatusSummary {
        self.inner.status_summary()
    }

    /// More rows exist only while the cap itself isn't reached — a capped
    /// view is complete at `cap`, so paging affordances stop there.
    fn has_more(&self) -> bool {
        self.inner.row_count() < self.cap && self.inner.has_more()
    }

    fn estimated_total(&self) -> Option<usize> {
        self.inner.estimated_total().map(|t| t.min(self.cap))
    }

    fn row(&self, idx: usize) -> Option<Arc<EnrichedRecord>> {
        if idx < self.cap {
            self.inner.row(idx)
        } else {
            None
        }
    }

    fn set_viewport(&self, range: Range<usize>) {
        self.inner
            .set_viewport(range.start.min(self.cap)..range.end.min(self.cap));
    }

    fn request_load_more(&self) {
        if self.inner.row_count() < self.cap {
            self.inner.request_load_more();
        }
    }

    fn request_refresh(&self) {
        self.inner.request_refresh();
    }

    fn set_search(&self, query: Option<String>) {
        self.inner.set_search(query);
    }

    fn set_sort(&self, column: Option<String>, dir: SortDir) {
        self.inner.set_sort(column, dir);
    }

    fn subscribe(&self) -> watch::Receiver<Generation> {
        self.inner.subscribe()
    }

    fn master_capabilities(&self) -> &VistaCapabilities {
        self.inner.master_capabilities()
    }

    fn demanded_columns(&self) -> Option<Vec<String>> {
        self.inner.demanded_columns()
    }
}
