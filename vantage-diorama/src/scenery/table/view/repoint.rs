//! Moving a view to the rows of its new query.
//!
//! Rows another view of that query already reads are joined as they are;
//! otherwise the view gets a copy of the rows it leaves, which its new
//! query's load replaces in place. The rows it leaves are never reshaped:
//! other views of the old query keep reading them, and when it was their
//! last view their work is aborted and they drop.

use std::sync::Arc;

use vantage_core::Priority;

use crate::dio::{DioInner, shared_key};
use crate::scenery::table::state::{Init, SharedRows};
use crate::scenery::table::{ViewportRequest, loader};

use super::{TableView, ViewHook};

/// What moved the view's query, as far as its new rows care.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Change {
    /// The order or the search.
    Clause,
    /// The runtime filters: a copy's total counted the old filters' set.
    Filters,
    /// A tail turning flipped: the copied rows sit in the other order, so
    /// they are dropped rather than shown until the reload lands.
    Flip,
}

impl TableView {
    /// Move the view to the rows of its current query: the live rows of that
    /// query when another view reads them, else a copy of the rows it leaves,
    /// reloaded for the new query. The caller holds `repointing`, so two
    /// setters racing on one view move it once each, in turn.
    pub(super) fn repoint(&self, change: Change) {
        let Some(dio) = self.dio_weak.upgrade() else {
            return;
        };
        let old = self.shared();
        let query = self.view_query();
        let key = shared_key(&dio, &query, self.titles_only);
        if key == old.key {
            // The same rows answer it; only the view's own clauses moved
            // (a condition restated between `filter` and `set_filters`).
            self.signal.bump_stats();
            return;
        }
        // Leaving the old rows aborts their work when this was their last
        // view: a load in flight for the old query never lands anywhere.
        let mut hook = old.detach(self.id).unwrap_or_else(|| self.hook());
        let viewport = hook.viewport.clone();
        if !self.sugared
            && let Some(live) = dio.shared_rows(&key)
        {
            match self.attach(&live, hook) {
                Ok(()) => return self.point_at(live, true),
                // Their last view left in between: build our own.
                Err(back) => hook = back,
            }
        }
        let init = Init {
            key: key.clone(),
            query,
            base: self.base.clone(),
            page_size: self.page_size,
            titles_only: self.titles_only,
            paged: old.paged,
            read_cache: old.read_cache.clone(),
            sugar_columns: old.sugar_columns.clone(),
        };
        let (copy, reloaded) = SharedRows::copied_from(&old, &dio, init, (self.id, hook));
        let (rows, joined) = if self.sugared {
            (copy, false)
        } else {
            self.publish(&dio, key, copy)
        };
        if !joined {
            match change {
                Change::Filters => {
                    rows.set_total(None);
                }
                Change::Flip => rows.drop_rows(),
                Change::Clause => {}
            }
            if !reloaded {
                self.reload(&rows, viewport.is_some());
            }
        }
        // A reload the change of serving ran inside `copied_from` bumped
        // before the swap, so it bumps again once the view points here; so
        // does a flip, whose dropped copy shows nothing until it reloads.
        self.point_at(rows, joined || reloaded || change == Change::Flip);
    }

    /// Register `copy` under `key`, or join the rows a concurrent re-point
    /// registered there first (and let the copy drop). Returns the rows to
    /// point at and whether they were joined.
    fn publish(
        &self,
        dio: &DioInner,
        key: String,
        copy: Arc<SharedRows>,
    ) -> (Arc<SharedRows>, bool) {
        loop {
            let rows = dio.register_shared_rows(key.clone(), copy.clone());
            if Arc::ptr_eq(&rows, &copy) {
                return (rows, false);
            }
            // Not `detach`: the copy stays ours, with its work, unless the
            // join below succeeds.
            let hook = copy.views.lock().unwrap().remove(&self.id);
            match self.attach(&rows, hook.unwrap_or_else(|| self.hook())) {
                Ok(()) => return (rows, true),
                // Their last view left in between; the next registration
                // replaces them with the copy.
                Err(hook) => {
                    copy.views.lock().unwrap().insert(self.id, hook);
                }
            }
        }
    }

    /// Swap the view onto `rows` and tell its subscribers. Joining live rows
    /// changes what the view shows at once, so `rows_moved` bumps its
    /// generation; a copy shows the rows the view already had, and its
    /// reload bumps when the new query's rows land.
    fn point_at(&self, rows: Arc<SharedRows>, rows_moved: bool) {
        *self.shared.write().unwrap() = rows;
        if rows_moved {
            self.signal.bump();
        }
        self.signal.bump_stats();
    }

    /// Join rows another view already keeps current, or hand the hook back
    /// when their last view has left. Nothing reloads; a viewport the view
    /// had is asked of them, which costs nothing when they hold it already.
    fn attach(&self, rows: &SharedRows, hook: ViewHook) -> Result<(), ViewHook> {
        let viewport = hook.viewport.clone();
        rows.try_attach(self.id, hook)?;
        if let Some(range) = viewport {
            loader::enqueue_viewport(
                rows,
                ViewportRequest {
                    range,
                    force_load: false,
                    priority: Priority::Essential,
                    view: Some(self.id),
                },
            );
        }
        Ok(())
    }

    /// Reload a fresh copy for the new query. A view serving windows
    /// refetches what it shows in place (the first page when it has asked
    /// for none yet); Essential, since the rows on screen answer the previous
    /// query and the user is waiting on this one. Any other view re-derives
    /// its rows on the reactor: a cache reseed, or a two-pass resort.
    fn reload(&self, rows: &SharedRows, has_viewport: bool) {
        if !rows.is_chunk_loaded() {
            rows.reload_notify.notify_one();
        } else if has_viewport {
            rows.refresh_loaded_viewport(Priority::Essential);
        } else {
            loader::enqueue_viewport(
                rows,
                ViewportRequest {
                    range: 0..self.page_size,
                    force_load: true,
                    priority: Priority::Essential,
                    view: Some(self.id),
                },
            );
        }
    }
}
