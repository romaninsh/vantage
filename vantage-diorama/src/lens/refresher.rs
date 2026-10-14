//! [`Refresher`]: how, and how often, a Lens re-reads a Dio's master.

use std::future::Future;
use std::time::Duration;

use vantage_core::{Result, error};

use super::callbacks::{CxFn, boxed_cx};
use super::loader_cx::LoaderCx;

/// Re-reads a Dio's master into its cache.
///
/// The refresh fn runs on [`Dio::refresh`](crate::Dio::refresh) and, with
/// [`every`](Self::every), on a timer. An augmented Dio runs its augmentation
/// refresh pass first, then the fn. Without a refresher a Lens has no timer.
///
/// [`max_age`](Self::max_age) bounds how stale a cache may be when it is
/// opened: a Dio whose last successful refresh is older (or unrecorded) gets a
/// background refresh when it is built and whenever a table or record scenery
/// opens on it. The time is persisted in the cache, so it holds across
/// restarts.
pub struct Refresher {
    pub(crate) refresh: CxFn,
    pub(crate) every: Option<Duration>,
    pub(crate) max_age: Option<Duration>,
}

impl Refresher {
    pub fn new<F, Fut>(f: F) -> Self
    where
        F: Fn(LoaderCx) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self {
            refresh: boxed_cx(f),
            every: None,
            max_age: None,
        }
    }

    /// Run the refresh on a timer, `interval` after the Dio is built and then
    /// every `interval`. Must be non-zero.
    pub fn every(mut self, interval: Duration) -> Self {
        self.every = Some(interval);
        self
    }

    /// The oldest a cache may be when it is opened before a background refresh
    /// is requested. Works alone: staleness is checked at build and at open,
    /// and no timer runs without [`every`](Self::every).
    pub fn max_age(mut self, ttl: Duration) -> Self {
        self.max_age = Some(ttl);
        self
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.every == Some(Duration::ZERO) {
            return Err(error!(
                "Refresher::every needs a non-zero interval",
                setting = "refresher.every"
            ));
        }
        Ok(())
    }
}
