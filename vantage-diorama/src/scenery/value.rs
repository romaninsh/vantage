//! `ValueScenery` — reactive single-scalar view.
//!
//! One [`Aggregation`] over the rows the Dio admits, recomputed when the Dio
//! changes. It suits any "one number that refreshes" shape: a badge, a
//! dashboard counter. Open one with
//! [`Dio::value_scenery`](crate::Dio::value_scenery).
//!
//! Status + value transitions:
//!
//! | Trigger                              | Status     | Value                  | Bump |
//! |--------------------------------------|------------|------------------------|------|
//! | Open, rows read                      | `Fresh`    | computed               | yes  |
//! | Open, reading the rows failed        | `Error(_)` | `None`                 | yes  |
//! | Recompute, unchanged                 | unchanged  | unchanged              | no   |
//! | Recompute, changed                   | `Fresh`    | new                    | yes  |
//! | Reading the rows failed              | `Error(_)` | **last good preserved**| yes  |
//! | First good read after a failure      | `Fresh`    | computed               | yes  |
//!
//! The aggregation runs over the Dio's cache; nothing is asked of the master.

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
