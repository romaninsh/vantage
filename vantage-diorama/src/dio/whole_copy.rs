//! Copying the whole set into the cache on demand, for a paged view whose
//! query leaves a clause the master can't apply.

use std::sync::Arc;

use tokio::sync::watch;
use vantage_core::{Result, error};

use super::{Dio, DioInner};
use crate::lens::LoaderCx;

/// The outcome of the one whole copy a Dio runs: `None` while it runs, then
/// `Ok` or the start's error message.
pub(crate) type CopyOutcome = Option<std::result::Result<(), String>>;

impl DioInner {
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
                dio.inner.stamp_refreshed().await;
            }
            let _ = tx.send(Some(outcome.map_err(|e| e.to_string())));
        });
        Ok(rx)
    }
}
