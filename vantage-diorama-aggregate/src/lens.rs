//! `AggregateLens` — the aggregate datasource.
//!
//! A derived Dio's needs are the inverse of a source Dio's: there is no master
//! to fetch from, nothing to write back, and its contents are reproducible from
//! the source at any time. This Lens encodes exactly that, and owns **one cache
//! file for every aggregation** — each aggregate claiming a named table within
//! it, the same shape a datasource uses for its tables.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use ciborium::Value as CborValue;
use tokio::sync::Notify;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{CacheBackend, Dio, Lens, MemoryCache, RedbCache, ValueScenery};
use vantage_vista::Vista;

use crate::aggregation::{AggregateOutput, Aggregation, DerivedRows};
use crate::derived::{AggregateShell, DerivedDio, DerivedState, shared};
use crate::engine::{self, Publish, Source};
use crate::value::{AggregateValue, TaskGuard, ValueState};

/// How long to hold off after reacting, before flushing whatever else arrived.
const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(250);

pub struct AggregateLensBuilder {
    backend: Arc<dyn CacheBackend>,
    debounce: Duration,
}

impl AggregateLensBuilder {
    /// Cooldown after each recomputation.
    ///
    /// The first change in a burst is always acted on immediately; this bounds
    /// how often the ones behind it are. Whatever arrives during the cooldown
    /// is flushed once at the end, never dropped.
    pub fn debounce(mut self, window: Duration) -> Self {
        self.debounce = window;
        self
    }

    pub fn build(self) -> Result<Arc<AggregateLens>> {
        let lens = Lens::new()
            .cache_source(self.backend.clone())
            // Nothing upstream to re-fetch: this Lens's Dios are fed by the
            // aggregation engine, not by a master.
            .refresh_on_open(false)
            .build()
            .map_err(|e| vantage_core::error!("aggregate lens", detail = e.to_string()))?;
        Ok(Arc::new(AggregateLens {
            lens: Arc::new(lens),
            backend: self.backend,
            debounce: self.debounce,
        }))
    }
}

pub struct AggregateLens {
    lens: Arc<Lens>,
    backend: Arc<dyn CacheBackend>,
    debounce: Duration,
}

impl AggregateLens {
    /// Persist derived results at `path` — one file, one table per aggregate.
    pub fn at(path: impl AsRef<Path>) -> Result<AggregateLensBuilder> {
        Ok(AggregateLensBuilder {
            backend: Arc::new(RedbCache::open(path)?),
            debounce: DEFAULT_DEBOUNCE,
        })
    }

    /// Keep derived results in memory only. Aggregates are reproducible from
    /// their source, so this loses nothing but the warm start.
    pub fn in_memory() -> AggregateLensBuilder {
        AggregateLensBuilder {
            backend: Arc::new(MemoryCache::new()),
            debounce: DEFAULT_DEBOUNCE,
        }
    }

    /// Reuse a backend already opened elsewhere — e.g. to put aggregates in the
    /// same file as the datasource they derive from.
    pub fn with_backend(backend: Arc<dyn CacheBackend>) -> AggregateLensBuilder {
        AggregateLensBuilder {
            backend,
            debounce: DEFAULT_DEBOUNCE,
        }
    }

    /// Close the aggregate cache cleanly, before the process exits.
    ///
    /// Derived results are reproducible, so losing this file costs only a warm
    /// start — but an unclean close costs a full-file repair scan on every
    /// launch afterwards, which is worse than the thing the cache buys.
    pub async fn close(&self) {
        self.backend.close().await;
    }

    /// Drop cached aggregates whose names are no longer in use.
    ///
    /// Names encode what an aggregate *is*; when a definition changes, the old
    /// table would otherwise sit there forever. Backends that cannot enumerate
    /// their tables report none, so this is a no-op on them.
    pub async fn retain(&self, keep: &[&str]) -> Result<()> {
        for name in self.backend.list_tables().await? {
            if keep.contains(&name.as_str()) {
                continue;
            }
            tracing::debug!(
                target: "vantage_diorama_aggregate",
                table = %name,
                "dropping unused aggregate cache",
            );
            self.backend.drop_table(&name).await?;
        }
        Ok(())
    }

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
                reader: source.vista(),
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
    /// drive a grid from it. Its Vista advertises `can_order` / `can_search` /
    /// `can_count` and honours them, because unlike its source it holds its
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
        let tap = source.debug_tap();
        let start = tap.enabled().then(std::time::Instant::now);
        let rows = source.vista().list_values().await?;
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
        let vista = Vista::new(name, Box::new(shell));
        let dio = self.lens.make_dio_as(vista, name.to_string()).await?;

        let state = DerivedState::new(shared_rows, dio.clone());
        // Fill the cache before handing the Dio back, so a scenery opened on it
        // paints immediately instead of waiting for the first recomputation.
        state.publish(initial.clone()).await;

        let handle = engine::spawn(
            name,
            Source {
                reader: source.vista(),
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
