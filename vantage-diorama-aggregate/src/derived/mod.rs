//! The row-set surface: an aggregation producing [`DerivedRows`] becomes a
//! full `Dio`, with its own Vista, its own cache table and its own sceneries.

mod shell;

use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use vantage_diorama::Dio;

use crate::aggregation::DerivedRows;
use crate::engine::Publish;

pub(crate) use shell::AggregateShell;
pub(crate) use shell::SharedRows;

use crate::value::TaskGuard;

pub(crate) fn shared(initial: DerivedRows) -> SharedRows {
    Arc::new(RwLock::new(initial))
}

/// A `Dio` fed by an aggregation.
///
/// Derefs to `Dio`, so everything downstream — `table_scenery()`, `vista()`,
/// `subscribe_events()` — works unchanged. Holding it is what keeps the
/// aggregation running: drop it and the recomputation task stops, which is the
/// same "release it and it stops pulling" contract sceneries follow.
pub struct DerivedDio {
    dio: Dio,
    _state: Arc<DerivedState>,
    _guard: TaskGuard,
}

impl DerivedDio {
    pub(crate) fn new(dio: Dio, state: Arc<DerivedState>, guard: TaskGuard) -> Self {
        Self {
            dio,
            _state: state,
            _guard: guard,
        }
    }

    /// The underlying Dio. Cloning it detaches the clone from this handle's
    /// lifetime — the aggregation still stops when the `DerivedDio` drops.
    pub fn dio(&self) -> &Dio {
        &self.dio
    }
}

impl std::ops::Deref for DerivedDio {
    type Target = Dio;

    fn deref(&self) -> &Self::Target {
        &self.dio
    }
}

/// Publishes a recomputed row set into the derived Dio.
pub(crate) struct DerivedState {
    rows: SharedRows,
    dio: Dio,
}

impl DerivedState {
    pub(crate) fn new(rows: SharedRows, dio: Dio) -> Arc<Self> {
        Arc::new(Self { rows, dio })
    }
}

#[async_trait]
impl Publish<DerivedRows> for DerivedState {
    async fn publish(&self, output: DerivedRows) {
        // The shell serves reads straight from here, so swap first: a Vista
        // read racing this sees either the old set or the new one, never a
        // half-built one.
        *self.rows.write().expect("aggregate rows poisoned") = output.clone();

        // Reconcile the cache row by row rather than clearing and refilling.
        // A clear would leave a window in which an open scenery reseeding from
        // the cache sees nothing — a visible blank on every recomputation.
        let cache = self.dio.cache();
        let existing = match cache.list_values().await {
            Ok(existing) => existing,
            Err(e) => {
                tracing::warn!(
                    target: "vantage_diorama_aggregate",
                    error = %e,
                    "failed to read derived cache; skipping this reconcile",
                );
                return;
            }
        };

        for (id, record) in &output.rows {
            if existing.get(id) == Some(record) {
                continue;
            }
            if let Err(e) = cache.insert_value(id, record).await {
                tracing::warn!(
                    target: "vantage_diorama_aggregate",
                    id = %id,
                    error = %e,
                    "failed to write derived row",
                );
            }
        }

        for id in existing.keys() {
            if output.rows.iter().any(|(row_id, _)| row_id == id) {
                continue;
            }
            if let Err(e) = cache.delete_value(id).await {
                tracing::warn!(
                    target: "vantage_diorama_aggregate",
                    id = %id,
                    error = %e,
                    "failed to remove derived row",
                );
            }
        }

        // Membership may have moved (a group appeared or emptied), so the
        // honest signal is the whole-set one.
        self.dio.notify_dataset_changed();
    }

    fn fail(&self, error: String) {
        // Leave the last good rows in place; a failed read of the source is
        // not a reason to empty a dashboard.
        tracing::warn!(
            target: "vantage_diorama_aggregate",
            error = %error,
            "aggregate source read failed; keeping previous rows",
        );
    }
}
