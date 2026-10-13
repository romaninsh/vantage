//! Mounting an aggregation over a source Dio, as a value or as a derived Dio.
//!
//! Both read the source through its facade, [`Dio::cached`], so an
//! aggregate over a narrowed Dio reduces only the rows that Dio admits.

use std::sync::Arc;

use ciborium::Value as CborValue;
use tokio::sync::Notify;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Dio, ValueScenery};
use vantage_vista::Vista;

use super::AggregateLens;
use crate::aggregation::{AggregateOutput, Aggregation, DerivedRows};
use crate::derived::{AggregateShell, DerivedDio, DerivedState, shared};
use crate::engine::{self, Publish, Source};
use crate::value::{AggregateValue, TaskGuard, ValueState};

impl AggregateLens {
    /// Mount a scalar aggregation as a [`ValueScenery`] — the shape a UI binds
    /// to a counter or a total.
    ///
    /// `name` identifies the aggregate's cache table, so it must be stable
    /// across runs and distinct per aggregate.
    pub async fn value<A>(
        self: &Arc<Self>,
        source: &Dio,
        name: &str,
        aggregation: A,
    ) -> Result<Arc<dyn ValueScenery>>
    where
        A: Aggregation<Output = CborValue>,
    {
        let cache = self.backend.open_table(name).await?;
        let nudge = Arc::new(Notify::new());
        let state = ValueState::new(source.clone(), cache, nudge.clone());
        state.seed_from_cache().await;

        let handle = engine::spawn(
            name,
            Source {
                reader: source.cached(),
                dio: source.clone(),
            },
            Arc::new(aggregation),
            &state,
            self.debounce,
            nudge,
            None,
        );

        Ok(Arc::new(AggregateValue {
            state,
            _guard: TaskGuard(handle),
        }))
    }

    /// Mount a **scalar** reduction as a one-row Dio — the shape SQL gives a
    /// `count(*)`, and the shape every consumer here already handles.
    ///
    /// Prefer this to [`value`](Self::value) when the result is going to be
    /// observed like data. `value` hands back a bare number, which forces a
    /// parallel scenery/observation path all the way up the stack; this hands
    /// back a table with one row and one column named `alias`, so a grid, a
    /// chart and a single-figure tile all bind to it identically.
    ///
    /// ```ignore
    /// let opened = agg.derive_value(&events, "events#count[Event=opened]", "opened",
    ///                               catalog.build("count", spec)?).await?;
    /// // → a Dio whose whole content is:  opened
    /// //                                  ------
    /// //                                       7
    /// ```
    pub async fn derive_value(
        self: &Arc<Self>,
        source: &Dio,
        name: &str,
        alias: &str,
        reduction: crate::ScalarAggregation,
    ) -> Result<DerivedDio> {
        self.derive(source, name, crate::AsRows::new(alias, reduction))
            .await
    }

    /// Mount a row-producing aggregation as its own `Dio`.
    ///
    /// The result is an ordinary Dio: open sceneries on it, sort and search it,
    /// drive a grid from it. Its Vista advertises `Order`, `Search` and
    /// `Window` and honours them, because unlike its source it holds its
    /// entire row set.
    pub async fn derive<A>(
        self: &Arc<Self>,
        source: &Dio,
        name: &str,
        aggregation: A,
    ) -> Result<DerivedDio>
    where
        A: Aggregation<Output = DerivedRows>,
    {
        // Compute once up front: the schema of the derived table is whatever
        // the aggregation declares, and the Vista needs it before it exists.
        let aggregation = Arc::new(aggregation);
        let reader = source.cached();
        let tap = source.debug_tap();
        let start = tap.enabled().then(std::time::Instant::now);
        let rows = reader.list_values().await?;
        let initial = aggregation.compute(&rows);

        // This eager pass never goes through the engine's own `recompute` —
        // it runs before `engine::spawn` even exists to seed the Vista's
        // schema — so it carries its own debug line. The engine's Seed pass
        // that follows will report the same output as `unchanged` (it is
        // seeded with `already_published` below), which is the intended
        // "publish skipped" reading for that second line.
        //
        // rows_in/rows_out stay inside this gate, not hoisted above it, so
        // nothing here costs anything when the tap is off.
        if tap.enabled() {
            let ms = start.map(|s| s.elapsed().as_millis()).unwrap_or(0);
            tracing::info!(
                target: "vantage_diorama::debug",
                "{:<10} {:<8} \"{}\" recomputed from {} rows → {} in {} (initial, published)",
                tap.ds(),
                "derive",
                name,
                vantage_diorama::debug::num(rows.len()),
                vantage_diorama::debug::num(initial.debug_row_count()),
                vantage_diorama::debug::dur(ms as u64),
            );
        }

        let shared_rows = shared(initial.clone());
        let shell = AggregateShell::new(shared_rows.clone(), &initial.columns);
        let dio = self.lens.dio(Vista::new(name, shell)).build().await?;

        let state = DerivedState::new(shared_rows, dio.clone());
        // Fill the cache before handing the Dio back, so a scenery opened on it
        // paints immediately instead of waiting for the first recomputation.
        state.publish(initial.clone()).await;

        let handle = engine::spawn(
            name,
            Source {
                reader,
                dio: source.clone(),
            },
            aggregation,
            &state,
            self.debounce,
            Arc::new(Notify::new()),
            Some(initial),
        );

        Ok(DerivedDio::new(dio, state, TaskGuard(handle)))
    }
}
