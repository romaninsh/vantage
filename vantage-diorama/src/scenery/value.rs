//! `ValueScenery`: one [`Aggregation`] over the rows the Dio admits, kept
//! current as they change: a badge, a dashboard counter.
//!
//! Opened with [`Dio::value_scenery`](crate::Dio::value_scenery). It reads the
//! Dio's cache, never the master. A changed value bumps the generation and an
//! unchanged one doesn't; a failed read reads `Error(_)` and keeps the last
//! good value until a read succeeds again.

use std::sync::Arc;

use ciborium::Value as CborValue;
use tokio::sync::watch;
use vantage_core::Result;

use crate::aggregate::{Aggregation, open_value};
use crate::dio::{Dio, Generation};

#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ValueStatus {
    Loading,
    Fresh,
    Error(String),
}

/// Reactive view onto a single scalar — typically an aggregate
/// (`COUNT`, `SUM`) computed against the underlying Dio.
pub trait ValueScenery: Send + Sync {
    fn value(&self) -> Option<CborValue>;
    fn status(&self) -> ValueStatus;

    /// Re-read the Dio's set from its source; the value recomputes when it
    /// lands.
    fn refresh_source(&self);
    fn subscribe(&self) -> watch::Receiver<Generation>;
}

/// Builder produced by [`Dio::value_scenery`](crate::Dio::value_scenery).
pub struct ValueSceneryBuilder<A> {
    dio: Dio,
    aggregation: A,
}

impl<A> ValueSceneryBuilder<A> {
    pub(crate) fn new(dio: Dio, aggregation: A) -> Self {
        Self { dio, aggregation }
    }
}

impl<A: Aggregation<Output = CborValue>> ValueSceneryBuilder<A> {
    /// Compute the value once, then keep it current as the Dio changes.
    pub async fn open(self) -> Result<Arc<dyn ValueScenery>> {
        open_value(self.dio, self.aggregation).await
    }
}
