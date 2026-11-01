//! Refreshing a Dio from its master: on request, on the Lens's timer, and
//! when the cache is older than the refresher's `max_age`.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, SystemTime};

use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet as _;

use super::augment_passes::{self, RefreshKind};
use super::{Dio, DioEvent, DioInner};
use crate::lens::LoaderCx;

impl Dio {
    /// Re-read the master: an augmented Dio runs its augmentation refresh
    /// pass, then the Lens's [`Refresher`](crate::Refresher) runs, if it has
    /// one. Publishes `DatasetChanged` on success and records the refresh time
    /// in the cache. Errors propagate (the timer only logs them).
    ///
    /// With neither, the set is still re-read the way it was loaded: a Dio
    /// whose views page their windows publishes `DatasetChanged`, so each
    /// view re-fetches its window and restates the total (and a view served
    /// from a whole copy copies again); a Dio the Lens copies whole re-runs
    /// the loader's eager start; any other Dio
    /// [`reconcile`](Self::reconcile)s its cache with `master.list()`. This is
    /// how a live follow recovers after an `Invalidated` change or a lagged
    /// stream.
    ///
    /// A refresher that calls [`LoaderCx::reconcile`] has already recorded
    /// the refresh and published `DatasetChanged`; the refresh doesn't do
    /// either again.
    ///
    /// This is the *requested* path: an augmented Dio re-probes rows that
    /// previously found no detail record. The timer takes the scheduled path,
    /// which doesn't — a timed re-pull of the master is no evidence the detail
    /// source changed.
    pub async fn refresh(&self) -> Result<()> {
        self.refresh_with(RefreshKind::Requested).await
    }

    /// The timer's refresh: [`Self::refresh`], except that settled-empty
    /// augmentation rows are left alone, so a detail source that has no
    /// record for a row isn't re-queried every tick.
    pub(crate) async fn refresh_scheduled(&self) -> Result<()> {
        self.refresh_with(RefreshKind::Scheduled).await
    }

    async fn refresh_with(&self, kind: RefreshKind) -> Result<()> {
        let _ = self.inner.event_bus.send(DioEvent::Refreshing);
        let augmented = self.inner.has_dio_augment();
        let refresher = self.inner.lens.callbacks.refresher.as_ref();
        if !augmented && refresher.is_none() {
            return self.reread().await;
        }
        if augmented {
            augment_passes::refresh(self, kind).await?;
        }
        let mut reconciled = false;
        if let Some(refresher) = refresher {
            let cx = LoaderCx::new(self.clone());
            (refresher.refresh)(cx.clone()).await?;
            reconciled = cx.reconciled();
        }
        // The augmentation refresh pass reconciles the whole set with the
        // master; a refresher alone may have written any part of it.
        self.announce_refresh(reconciled, augmented).await;
        Ok(())
    }

    /// The refresh of a Dio with no refresher and no augmentation: re-read
    /// the set the way it was loaded.
    ///
    /// - A Dio whose views page their windows publishes `DatasetChanged`, so
    ///   each view re-fetches; a finished whole copy is forgotten, so a view
    ///   served from it copies again.
    /// - A Dio the Lens copies whole re-runs the loader's eager start.
    /// - Any other Dio reconciles with `master.list()`.
    async fn reread(&self) -> Result<()> {
        let inner = &self.inner;
        let loader = &inner.lens.callbacks.loader;
        if loader.pages(&inner.master.capabilities()) {
            inner.forget_whole_copy();
            self.notify_dataset_changed();
            return Ok(());
        }
        if let Some(start) = loader.eager.as_ref() {
            let cx = LoaderCx::new(self.clone());
            start(cx.clone()).await?;
            self.announce_refresh(cx.reconciled(), true).await;
            return Ok(());
        }
        let mark = inner.pending_flashes.mark();
        let snapshot = inner.master.list_values().await?;
        self.reconcile_since(&mark, snapshot).await
    }

    /// Record the refresh time, and the whole copy when the refresh wrote
    /// one (`whole`), then publish `DatasetChanged`, unless the callback
    /// already did all of it by reconciling.
    async fn announce_refresh(&self, reconciled: bool, whole: bool) {
        if reconciled {
            return;
        }
        if whole {
            self.inner.stamp_whole().await;
        } else {
            self.inner.stamp_refreshed().await;
        }
        self.notify_dataset_changed();
    }
}

impl DioInner {
    /// Record that the cache now matches the master. A failure to record is
    /// logged, not returned: the refresh it describes did succeed, and the
    /// cost of a lost stamp is one extra refresh on the next open.
    pub(crate) async fn stamp_refreshed(&self) {
        if let Err(e) = self.cache.set_meta_refreshed_at(SystemTime::now()).await {
            tracing::warn!(
                target: "vantage_diorama::cache",
                table = %self.cache_table_name,
                error = %e,
                "recording the refresh time failed",
            );
        }
    }

    /// Request a background refresh when the Lens's refresher has a `max_age`
    /// and the cache's last refresh is older than it, or was never recorded.
    /// Never blocks the caller, and at most one check runs per Dio at a time.
    /// Skipped while a detached eager start is still copying: that start is
    /// the fresh load, and it records the time when it lands.
    pub(crate) fn refresh_if_stale(self: &Arc<Self>) {
        if !self.seed_complete.load(Ordering::SeqCst) {
            return;
        }
        let Some(max_age) = self.max_age() else {
            return;
        };
        if self.stale_check_pending.swap(true, Ordering::SeqCst) {
            return;
        }
        let dio = Dio {
            inner: self.clone(),
        };
        self.lens.runtime.spawn(async move {
            let stale = match dio.inner.cache.meta_refreshed_at().await {
                Ok(Some(at)) => outlived(at, max_age),
                Ok(None) => true,
                Err(e) => {
                    tracing::warn!(
                        target: "vantage_diorama::cache",
                        error = %e,
                        "reading the refresh time failed — refreshing",
                    );
                    true
                }
            };
            if stale && let Err(e) = dio.refresh().await {
                tracing::error!(error = %e, "refresh for max_age failed");
            }
            dio.inner.stale_check_pending.store(false, Ordering::SeqCst);
        });
    }

    /// The refresher's `max_age`, when the Lens has a refresher with one.
    pub(crate) fn max_age(&self) -> Option<Duration> {
        self.lens
            .callbacks
            .refresher
            .as_ref()
            .and_then(|r| r.max_age)
    }
}

/// Whether a refresh recorded at `at` is older than `max_age`. A stamp in the
/// future (the clock stepped back) counts as young.
pub(crate) fn outlived(at: SystemTime, max_age: Duration) -> bool {
    SystemTime::now()
        .duration_since(at)
        .is_ok_and(|age| age > max_age)
}
