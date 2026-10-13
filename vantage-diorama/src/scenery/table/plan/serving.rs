//! Where a view's answer comes from: windows read from the master, the
//! complete set in the cache, or nowhere.
//!
//! A window read from the master reflects only the clauses the master took.
//! A view whose query leaves anything over (a condition, a search or an
//! order the master refused, a clause on an augmented column, or the Dio's
//! own residual) is answered where that residual can be evaluated over a
//! complete set, or it fails. It never shows a window as the filtered set.

use std::sync::Arc;

use vantage_core::{Priority, VantageError};
use vantage_vista::VistaQuery;

use crate::dio::{DioEvent, DioInner};
use crate::scenery::table::{LoadFailure, ViewportRequest};

use super::super::state::TableSceneryState;
use super::ViewPlan;
use super::failures::{rejection, unevaluated, unhydrated};

/// How a view serves its current query.
#[derive(Debug, Clone)]
pub(crate) enum Serving {
    /// A paged view whose master takes the whole query: each window read
    /// from the master is the answer for its rows.
    Windows,
    /// Every clause is evaluated over the complete set held in the cache: an
    /// eager view; a two-pass view over its index, which a view with any
    /// clause lists to the end before it reads as anything but loading; or a
    /// paged view whose Lens copies the whole set through its eager start.
    Cache,
    /// The view can't answer its query and shows no rows.
    Failed(LoadFailure),
}

impl Serving {
    /// The serving for a view whose query offers `offered` to the master,
    /// planned as `planned`, and keeps `sugar` (clauses on sugar outputs)
    /// for the rows it reads.
    ///
    /// A query the master rejects fails every view. A titles-only picker
    /// never hydrates, so a clause on an augmented column, in its query or
    /// in the Dio's residual, fails it too.
    pub(crate) fn decide(
        state: &TableSceneryState,
        dio: &DioInner,
        offered: &VistaQuery,
        sugar: &VistaQuery,
        planned: Result<&ViewPlan, &VantageError>,
    ) -> Self {
        let plan = match planned {
            Ok(plan) => plan,
            Err(e) => return Serving::Failed(rejection(dio, offered, e)),
        };
        if state.two_pass
            && state.titles_only
            && let Some(failure) = unhydrated(dio, offered)
        {
            return Serving::Failed(failure);
        }
        if !state.paged || state.two_pass {
            return Serving::Cache;
        }
        if !plan.has_residual() && sugar.is_empty() {
            Serving::Windows
        } else if dio.lens.callbacks.loader.eager.is_some() {
            Serving::Cache
        } else {
            let mut residual = plan.residual().clone();
            residual.conditions.extend(sugar.conditions.iter().cloned());
            residual.order = residual.order.or_else(|| sugar.order.clone());
            Serving::Failed(unevaluated(dio, &residual))
        }
    }

    pub(crate) fn is_windows(&self) -> bool {
        matches!(self, Serving::Windows)
    }
}

impl TableSceneryState {
    /// The view's current serving.
    pub(crate) fn serving(&self) -> Serving {
        self.serving.read().unwrap().clone()
    }

    /// Act on the serving moving from `previous` to `next`. Returns whether
    /// the view was reloaded here, so the setter skips its own reload.
    ///
    /// - Failed: the rows are withheld and `LoadFailed` is emitted.
    /// - Back to windows: the rows, placed by the cache, are dropped, and the
    ///   last viewport is fetched from the master.
    /// - A paged view moving to the cache from windows or a failure: the rows
    ///   are dropped and the view reads as loading; the caller's reload
    ///   copies the whole set and refines it.
    /// - Any other view leaving a failure reads as loading until the caller's
    ///   reload lands.
    pub(crate) fn enter(&self, dio: &Arc<DioInner>, previous: &Serving, next: &Serving) -> bool {
        match next {
            Serving::Failed(failure) => {
                self.withhold(dio, failure);
                true
            }
            Serving::Windows if !previous.is_windows() => {
                self.drop_rows();
                let range = self
                    .last_viewport
                    .read()
                    .unwrap()
                    .clone()
                    .unwrap_or(0..self.page_size);
                super::super::loader::enqueue_viewport(
                    self,
                    ViewportRequest {
                        range,
                        force_load: true,
                        priority: Priority::Essential,
                    },
                );
                true
            }
            Serving::Cache
                if !matches!(previous, Serving::Cache) && self.paged && !self.two_pass =>
            {
                self.drop_rows();
                false
            }
            _ => {
                if matches!(previous, Serving::Failed(_)) {
                    self.unsettle();
                }
                false
            }
        }
    }

    /// Fail the view with `failure`, as when a whole copy it waited on
    /// failed.
    pub(crate) fn fail(&self, dio: &Arc<DioInner>, failure: LoadFailure) {
        *self.serving.write().unwrap() = Serving::Failed(failure.clone());
        self.withhold(dio, &failure);
    }

    /// Show no rows, and say why: on the bus as `LoadFailed`, in the load
    /// state as `Failed`.
    fn withhold(&self, dio: &DioInner, failure: &LoadFailure) {
        self.clear_rows();
        tracing::warn!(
            target: "vantage_diorama::viewport",
            table = %failure.table,
            clause = ?failure.clause,
            error = %failure.message,
            "the view can't answer its query; it shows no rows",
        );
        crate::debug::tapline!(self.debug_tap, "scenery", "failed — {}", failure);
        let range = self.last_viewport.read().unwrap().clone().unwrap_or(0..0);
        let _ = dio.event_bus.send(DioEvent::LoadFailed {
            range,
            error: failure.to_string(),
        });
        self.note_state("query can't be answered");
        self.bump_generation();
    }

    /// Forget the rows and the total of the previous serving: their places
    /// mean nothing under the new one. The view loads again from scratch and
    /// reads as loading; the load that refills it bumps the generation.
    fn drop_rows(&self) {
        self.settled
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.clear_rows();
    }

    fn clear_rows(&self) {
        self.rows.write().unwrap().clear();
        self.id_to_idx.write().unwrap().clear();
        self.set_total(None);
    }

    /// Whether the view has failed its current query, so nothing may put
    /// rows in front of the user.
    pub(crate) fn is_failed(&self) -> bool {
        matches!(*self.serving.read().unwrap(), Serving::Failed(_))
    }

    /// Whether the serving is still the one `began` was read as: same kind,
    /// and not failed. A load that awaited something checks this before it
    /// installs rows, since a setter may have moved the view on meanwhile.
    pub(crate) fn still_serving(&self, began: &Serving) -> bool {
        let now = self.serving.read().unwrap();
        std::mem::discriminant(&*now) == std::mem::discriminant(began)
            && !matches!(*now, Serving::Failed(_))
    }
}
