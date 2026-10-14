//! Mounting an aggregation over a Dio, as a value or as a derived Dio.
//!
//! The aggregation reduces the rows the Dio admits, so an aggregate over a
//! narrowed Dio reduces only the rows that Dio holds.

use std::sync::Arc;

use ciborium::Value as CborValue;
use tokio::sync::Notify;
use vantage_core::{Context as _, Result, error};
use vantage_vista::Vista;

use crate::aggregate::aggregation::{Aggregation, DerivedRows};
use crate::aggregate::derived::{AggregateShell, DerivedState, aggregate_rows};
use crate::aggregate::engine::{self, Publish};
use crate::aggregate::value::{AggregateValue, ValueState};
use crate::{Cache, Dio, Lens, ValueScenery, ValueSceneryBuilder};

impl Dio {
    /// Start a [`ValueScenery`] builder: `aggregation` over the rows this Dio
    /// admits, recomputed as they change. Call `.open().await` to mount it.
    ///
    /// Every `open()` mounts its own recomputation; two value sceneries of one
    /// aggregation share nothing but the source.
    ///
    /// ```ignore
    /// let open = dio
    ///     .value_scenery(Where::new(Conditions::new().eq("status", "open"), Count::rows()))
    ///     .open()
    ///     .await?;
    /// ```
    pub fn value_scenery<A: Aggregation<Output = CborValue>>(
        &self,
        aggregation: A,
    ) -> ValueSceneryBuilder<A> {
        ValueSceneryBuilder::new(self.clone(), aggregation)
    }

    /// A Dio over `aggregation`'s output, recomputed as this Dio's rows change.
    /// Dropping the returned Dio (every clone) stops the recomputation.
    ///
    /// The result is an ordinary, read-only Dio: open sceneries on it, sort and
    /// search it, drive a grid from it. Its Vista advertises `Order`, `Search`
    /// and `Window` and honours them, because unlike its source it holds its
    /// entire row set. A scalar mounts as a one-row table through
    /// [`AsRows`](crate::aggregate::AsRows).
    ///
    /// Every call mounts a new Dio on a Lens of its own, in memory, on this
    /// Dio's runtime and debug stream. Its Vista is named `<source>#aggregate`
    /// whatever the aggregation, so two aggregates of one source carry the same
    /// name, and are still two Dios: one never hands back the other's rows.
    ///
    /// ```ignore
    /// let by_status = orders.aggregate(GroupBy::column("status", revenue)).await?;
    /// let grid = by_status
    ///     .table_scenery()
    ///     .order("revenue", SortDirection::Descending)
    ///     .open()
    ///     .await?;
    /// ```
    pub async fn aggregate<A: Aggregation<Output = DerivedRows>>(
        &self,
        aggregation: A,
    ) -> Result<Dio> {
        let name = format!("{}#aggregate", self.master().name());
        // Compute once up front: the schema of the derived table is whatever
        // the aggregation declares, and the Vista needs it before it exists.
        let (bus, initial) = engine::compute_once(&name, self, &aggregation).await;
        let initial = initial?;

        let rows = aggregate_rows(initial.clone());
        let shell = AggregateShell::new(rows.clone(), &initial.columns);
        let source_lens = &self.inner.lens;
        // No loader, refresher or writer: the engine is this Dio's only feed.
        let lens = Lens::builder()
            .cache(Cache::memory())
            .runtime(source_lens.runtime.clone())
            .tuning(source_lens.tuning().clone().with_refresh_on_open(false))
            .tap(source_lens.debug.clone())
            .build()
            .context(error!("the aggregate lens failed to build"))?;
        let dio = lens.dio(Vista::new(name.as_str(), shell)).build().await?;

        let state = DerivedState::new(rows, dio.downgrade());
        // Fill the cache before handing the Dio back, so a scenery opened on it
        // paints immediately instead of waiting for the first recomputation.
        state.publish(initial.clone()).await;

        let task = engine::spawn(
            &name,
            self,
            Arc::new(aggregation),
            state,
            (bus, Arc::new(Notify::new())),
            Some(initial),
        );
        if dio.inner.owned_task.set(task).is_err() {
            return Err(error!("the aggregate Dio already owns a task", dio = name));
        }
        Ok(dio)
    }
}

/// Compute once, so the scenery opens holding a value, then follow the source.
///
/// Nothing is persisted: the source's cache is the warm start. A failed first
/// read opens the scenery in `Error`, and the loop recovers it on the next
/// change.
pub(crate) async fn open_value<A>(source: Dio, aggregation: A) -> Result<Arc<dyn ValueScenery>>
where
    A: Aggregation<Output = CborValue>,
{
    let name = format!("{}.value", source.master().name());
    let nudge = Arc::new(Notify::new());
    let state = ValueState::new(source.clone(), nudge.clone());
    let (bus, first) = engine::compute_once(&name, &source, &aggregation).await;
    let initial = match first {
        Ok(output) => {
            state.publish(output.clone()).await;
            Some(output)
        }
        Err(e) => {
            state.fail(e.to_string());
            None
        }
    };
    let task = engine::spawn(
        &name,
        &source,
        Arc::new(aggregation),
        state.clone(),
        (bus, nudge),
        initial,
    );
    Ok(Arc::new(AggregateValue { state, _task: task }))
}
