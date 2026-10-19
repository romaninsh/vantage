//! The cache as a whole copy of the Dio's set: copying it on demand, for a
//! paged view whose query leaves a clause the master can't apply, and
//! knowing when the cache already holds one.

use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use tokio::sync::watch;
use vantage_core::{Result, error};

use super::pending::SettleMark;
use super::refresh::outlived;
use super::{Dio, DioInner};
use crate::lens::LoaderCx;

/// The outcome of the one whole copy a Dio runs: `None` while it runs, then
/// `Ok` or the start's error message.
pub(crate) type CopyOutcome = Option<std::result::Result<(), String>>;

impl DioInner {
    /// Whether the cache holds the Dio's whole set, so a list pass may read
    /// it instead of the master: a whole copy was recorded
    /// ([`meta_copied_at`](crate::lens::CacheTable::meta_copied_at)) within
    /// the refresher's `max_age`, or, with no `max_age` to bound an earlier
    /// session's copy, since this Dio was built. Never while a detached
    /// eager start is still rewriting the cache. A stamp that can't be read
    /// counts as none.
    pub(crate) async fn holds_whole_copy(&self) -> bool {
        if !self.seed_complete.load(Ordering::SeqCst) {
            return false;
        }
        let Ok(Some(at)) = self.cache.meta_copied_at().await else {
            return false;
        };
        match self.max_age() {
            Some(max_age) => !outlived(at, max_age),
            None => at >= self.built_at,
        }
    }

    /// Record that the cache now holds the master's whole set, and so is
    /// refreshed too: every row of the set was written and every row outside
    /// it dropped. Logged on failure, as for
    /// [`stamp_refreshed`](Self::stamp_refreshed).
    pub(crate) async fn stamp_whole(&self) {
        self.stamp_refreshed().await;
        if let Err(e) = self.cache.set_meta_copied_at(SystemTime::now()).await {
            tracing::warn!(
                target: "vantage_diorama::cache",
                table = %self.cache_table_name,
                error = %e,
                "recording the whole copy failed",
            );
        }
    }

    /// Delete the cached rows among `cached` that `keep` doesn't hold: they
    /// vanished upstream. A row with a flash in flight, or one settled since
    /// `mark` (taken before the set was read), is left alone. Returns the
    /// ids it deleted.
    pub(crate) async fn drop_vanished<'a>(
        &self,
        cached: impl IntoIterator<Item = &'a String>,
        keep: &HashSet<String>,
        mark: &SettleMark,
    ) -> Result<Vec<String>> {
        let pending = &self.pending_flashes;
        let mut dropped = Vec::new();
        for id in cached {
            if !keep.contains(id) && !pending.guarded_since(mark, id) {
                self.cache.delete_value(id).await?;
                dropped.push(id.clone());
            }
        }
        Ok(dropped)
    }

    /// Record that the cache is a whole copy, after a list pass read the
    /// master's whole set into `listed`, every page of it after `mark`. Rows
    /// the cache holds outside it are dropped first, so the copy is honest.
    /// When that fails nothing is stamped, and the next open lists the master
    /// again.
    pub(crate) async fn stamp_whole_copy(&self, listed: &HashSet<String>, mark: &SettleMark) {
        let dropped = match self.cache.list_values().await {
            Ok(cached) => self.drop_vanished(cached.keys(), listed, mark).await,
            Err(e) => Err(e),
        };
        if let Err(e) = dropped {
            tracing::warn!(
                target: "vantage_diorama::cache",
                table = %self.cache_table_name,
                error = %e,
                "dropping vanished rows failed; the copy stays unstamped",
            );
            return;
        }
        self.stamp_whole().await;
    }

    /// Copy the whole set into the cache through the loader's eager start,
    /// once per Dio: every caller waits on the same copy. A copy that fails
    /// is forgotten once its waiters have the error, so a later call tries
    /// again.
    ///
    /// The copy runs as its own task, so a caller dropping its wait (a view
    /// closing mid-copy) never cancels it half-written.
    pub(crate) async fn copy_whole(self: &Arc<Self>) -> Result<()> {
        let mut outcome = self.whole_copy_outcome()?;
        let done = outcome
            .wait_for(Option::is_some)
            .await
            .map_err(|_| error!("the whole copy ended without an outcome"))?
            .clone();
        let Some(Err(message)) = done else {
            return Ok(());
        };
        let mut slot = self.whole_copy.lock().unwrap();
        if slot
            .as_ref()
            .is_some_and(|rx| matches!(*rx.borrow(), Some(Err(_))))
        {
            *slot = None;
        }
        Err(error!(
            "copying the whole set failed",
            table = self.master.name(),
            detail = message
        ))
    }

    /// Forget a finished whole copy, so the next view served from the cache
    /// copies the set again. A copy still running is left to finish.
    pub(crate) fn forget_whole_copy(&self) {
        let mut slot = self.whole_copy.lock().unwrap();
        if slot.as_ref().is_some_and(|rx| rx.borrow().is_some()) {
            *slot = None;
        }
    }

    /// The receiver of the running or finished copy, starting one when none
    /// is held.
    fn whole_copy_outcome(self: &Arc<Self>) -> Result<watch::Receiver<CopyOutcome>> {
        let mut slot = self.whole_copy.lock().unwrap();
        if let Some(rx) = slot.as_ref() {
            return Ok(rx.clone());
        }
        let Some(start) = self.lens.callbacks.loader.eager.as_ref() else {
            return Err(error!(
                "the loader has no eager start to copy the whole set",
                table = self.master.name()
            ));
        };
        let dio = Dio {
            inner: self.clone(),
        };
        let copy = start(LoaderCx::new(dio.clone()));
        let (tx, rx) = watch::channel(None);
        *slot = Some(rx.clone());
        self.lens.runtime.spawn(async move {
            let outcome = copy.await;
            if outcome.is_ok() {
                dio.inner.stamp_whole().await;
            }
            let _ = tx.send(Some(outcome.map_err(|e| e.to_string())));
        });
        Ok(rx)
    }
}
