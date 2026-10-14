//! The row-set surface: an aggregation producing [`DerivedRows`] becomes a
//! full `Dio`, with its own Vista, its own cache table and its own sceneries.

mod shell;

use std::sync::{Arc, RwLock};

use async_trait::async_trait;

use crate::WeakDio;
use crate::aggregate::aggregation::DerivedRows;
use crate::aggregate::engine::{Publish, Resume};

pub(crate) use shell::AggregateRows;
pub(crate) use shell::AggregateShell;

pub(crate) fn aggregate_rows(initial: DerivedRows) -> AggregateRows {
    Arc::new(RwLock::new(initial))
}

/// Publishes a recomputed row set into the derived Dio.
///
/// The Dio is held weakly: the recomputation feeds it but never keeps it
/// alive, so the Dio's last holder dropping is what stops the recomputation.
pub(crate) struct DerivedState {
    rows: AggregateRows,
    dio: WeakDio,
}

impl DerivedState {
    pub(crate) fn new(rows: AggregateRows, dio: WeakDio) -> Arc<Self> {
        Arc::new(Self { rows, dio })
    }
}

#[async_trait]
impl Publish<DerivedRows> for DerivedState {
    async fn publish(&self, output: DerivedRows) {
        let Some(dio) = self.dio.upgrade() else {
            return;
        };
        // The shell serves reads straight from here, so swap first: a Vista
        // read racing this sees either the old set or the new one, never a
        // half-built one.
        *self.rows.write().expect("aggregate rows poisoned") = output.clone();

        // Reconcile rather than clear and refill: a clear would leave a window
        // in which an open scenery reseeding from the cache sees nothing, a
        // visible blank on every recomputation. Membership may have moved (a
        // group appeared or emptied), so reconcile announces the whole set.
        if let Err(e) = dio.reconcile(output.rows.into_iter().collect()).await {
            tracing::warn!(
                error = %e,
                "failed to reconcile the derived cache",
            );
        }
    }

    /// Leave the last good rows in place; a failed read of the source is not
    /// a reason to empty a dashboard. Nothing shows the failure, so an
    /// identical set after it needs no reconcile.
    fn fail(&self, error: String) -> Resume {
        tracing::warn!(
            error = %error,
            "aggregate source read failed; keeping previous rows",
        );
        Resume::IfChanged
    }
}
