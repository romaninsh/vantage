//! The recomputation loop: one task per aggregate layer.
//!
//! Reactivity is **push**. The source Dio already turns everything that can
//! change its rows — a datasource live subscription, a refresh timer, a write,
//! an augment landing — into events on its bus. This loop listens to that bus
//! and recomputes; it never polls.

use std::sync::{Arc, Weak};
use std::time::Duration;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use tokio::sync::{Notify, broadcast};
use tokio::task::JoinHandle;
use vantage_dataset::traits::ReadableValueSet;
use vantage_diorama::{Dio, DioEvent};

use crate::aggregation::Aggregation;

/// Where a recomputed output goes. Implemented by the two surfaces.
#[async_trait]
pub(crate) trait Publish<O>: Send + Sync + 'static {
    /// Hand over an output that differs from the last published one.
    async fn publish(&self, output: O);

    /// Reading the source failed. The previous output stays visible — a
    /// transient read error should not blank a dashboard.
    fn fail(&self, error: String);
}

/// A source that can be read and watched.
pub(crate) struct Source<R> {
    /// Read port. Any [`ReadableValueSet`] over CBOR records will do; today
    /// this is the Dio's facade Vista.
    pub reader: R,
    /// Change notifications and the downward refresh path.
    pub dio: Dio,
}

/// Spawn the loop. It runs until the publisher is dropped, at which point the
/// task exits and releases its handle on the source.
///
/// `already_published` seeds the change detector for a caller that has computed
/// and published once itself (the derived-Dio path, which needs an output
/// before it can build its Vista) — so the loop's own first pass recognises
/// that output as current and stays quiet.
pub(crate) fn spawn<A, R, P>(
    source: Source<R>,
    aggregation: Arc<A>,
    publisher: &Arc<P>,
    debounce: Duration,
    nudge: Arc<Notify>,
    already_published: Option<A::Output>,
) -> JoinHandle<()>
where
    A: Aggregation,
    R: ReadableValueSet<Id = String, Value = CborValue> + Send + Sync + 'static,
    P: Publish<A::Output>,
{
    let publisher = Arc::downgrade(publisher);
    let runtime = tokio::runtime::Handle::current();
    runtime.spawn(run(
        source,
        aggregation,
        publisher,
        debounce,
        nudge,
        already_published,
    ))
}

async fn run<A, R, P>(
    source: Source<R>,
    aggregation: Arc<A>,
    publisher: Weak<P>,
    debounce: Duration,
    nudge: Arc<Notify>,
    already_published: Option<A::Output>,
) where
    A: Aggregation,
    R: ReadableValueSet<Id = String, Value = CborValue> + Send + Sync + 'static,
    P: Publish<A::Output>,
{
    let mut bus = source.dio.subscribe_events();
    let mut last: Option<A::Output> = already_published;

    // Seed: whatever the source already holds. A warm cache means the layer
    // has a value before any fetch completes.
    if recompute(&source, &aggregation, &publisher, &mut last)
        .await
        .is_break()
    {
        return;
    }

    loop {
        // Idle until something happens.
        match wait(&mut bus, &nudge).await {
            Wake::Changed => {}
            Wake::SourceGone => return,
        }

        // Leading edge: react at once, so a single change feels immediate.
        if recompute(&source, &aggregation, &publisher, &mut last)
            .await
            .is_break()
        {
            return;
        }

        // Cooldown: absorb the burst, then flush once. The trailing flush is
        // not optional — dropping whatever arrived during the cooldown would
        // strand the layer on a stale value with nothing left to wake it.
        let deadline = tokio::time::Instant::now() + debounce;
        let mut dirty = false;
        loop {
            match tokio::time::timeout_at(deadline, wait(&mut bus, &nudge)).await {
                Ok(Wake::Changed) => dirty = true,
                Ok(Wake::SourceGone) => return,
                Err(_) => break,
            }
        }
        if dirty
            && recompute(&source, &aggregation, &publisher, &mut last)
                .await
                .is_break()
        {
            return;
        }
    }
}

enum Wake {
    Changed,
    SourceGone,
}

/// Block until the source's rows may have changed.
///
/// The filter is otherwise deliberately permissive: over-triggering costs one
/// recomputation whose output comparison then suppresses the repaint, whereas
/// under-triggering leaves the layer silently wrong. A `Lagged` bus — events
/// dropped because this task fell behind — is simply another reason to
/// recompute, which is the whole benefit of holding no incremental state.
///
/// Two kinds of event are excluded, for different reasons.
///
/// **Loading events** (`ViewportChanged`, `LoadFailed`, `Hydrating`) describe a
/// viewport moving or a pass making progress, not rows changing. Two-pass
/// hydration emits `RecordChanged` for every row it fills in, so nothing is
/// missed by ignoring them.
///
/// `RangeLoaded` is NOT among them, though it reads like one. A single-pass
/// chunk load writes its rows through [`ChunkSink::push`], straight into the
/// cache — it emits no per-row event, and `RangeLoaded` is the only thing it
/// announces. Ignoring it meant a paged source could load its whole first page
/// and every aggregate over it would still be reporting the empty set it was
/// derived over: a dashboard opening on a cold cache showed three zeros and
/// kept showing them. It is also what makes an aggregate *progressive* — each
/// page that lands recomputes over everything held so far.
///
/// **`Refreshing`** announces that a refresh has *started*. It is emitted
/// before the source's `on_refresh` runs, and the common shape of that callback
/// is "clear the cache, then refill it" — so reacting here would read the
/// source mid-clear and publish a zero that the trailing `DatasetChanged`
/// immediately corrects. Waiting for that trailing event is what keeps a
/// refresh from flickering every derived value through empty. If the refresh
/// fails there is no `DatasetChanged` and the layer keeps its last good output,
/// which is the same stale-over-blank choice made everywhere else here.
async fn wait(bus: &mut broadcast::Receiver<DioEvent>, nudge: &Notify) -> Wake {
    loop {
        tokio::select! {
            _ = nudge.notified() => return Wake::Changed,
            received = bus.recv() => match received {
                Ok(DioEvent::ViewportChanged { .. })
                | Ok(DioEvent::LoadFailed { .. })
                | Ok(DioEvent::Hydrating { .. })
                | Ok(DioEvent::Refreshing) => continue,
                Ok(_) => return Wake::Changed,
                Err(broadcast::error::RecvError::Lagged(dropped)) => {
                    tracing::debug!(
                        target: "vantage_diorama_aggregate",
                        dropped,
                        "source event bus lagged — recomputing from the current rows",
                    );
                    return Wake::Changed;
                }
                Err(broadcast::error::RecvError::Closed) => return Wake::SourceGone,
            },
        }
    }
}

/// Read, compute, and publish only if the output moved.
///
/// Returns `Break` when the publisher has been dropped, which is how the task
/// learns to stop.
async fn recompute<A, R, P>(
    source: &Source<R>,
    aggregation: &Arc<A>,
    publisher: &Weak<P>,
    last: &mut Option<A::Output>,
) -> std::ops::ControlFlow<()>
where
    A: Aggregation,
    R: ReadableValueSet<Id = String, Value = CborValue> + Send + Sync + 'static,
    P: Publish<A::Output>,
{
    let Some(publisher) = publisher.upgrade() else {
        return std::ops::ControlFlow::Break(());
    };

    let rows = match source.reader.list_values().await {
        Ok(rows) => rows,
        Err(e) => {
            publisher.fail(e.to_string());
            return std::ops::ControlFlow::Continue(());
        }
    };

    let output = aggregation.compute(&rows);

    // The comparison is what makes unconditional recomputation affordable:
    // recomputing often is fine as long as it does not repaint often.
    if last.as_ref() == Some(&output) {
        return std::ops::ControlFlow::Continue(());
    }

    publisher.publish(output.clone()).await;
    *last = Some(output);
    std::ops::ControlFlow::Continue(())
}
