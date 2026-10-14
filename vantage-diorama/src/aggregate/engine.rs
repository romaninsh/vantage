//! The recomputation loop: one task per mounted aggregation.
//!
//! Reactivity is **push**. The source Dio already turns everything that can
//! change its rows — a datasource live subscription, a refresh timer, a write,
//! an augment landing — into events on its bus. This loop listens to that bus
//! and recomputes; it never polls.

mod wake;

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::sync::{Notify, broadcast};
use vantage_core::Result;

use crate::aggregate::aggregation::sealed::Sealed;
use crate::aggregate::aggregation::{Aggregation, DebugRowCount};
use crate::aggregate::source::{admitted_rows, cached_count};
use crate::debug::{DebugTap, dur, num, tapline};
use crate::dio::AbortOnDrop;
use crate::{Dio, DioEvent};
use wake::{Wake, wait};

/// Where a recomputed output goes. Implemented by the two surfaces.
#[async_trait]
pub(crate) trait Publish<O>: Send + Sync + 'static {
    /// Hand over an output that differs from the last published one.
    async fn publish(&self, output: O);

    /// Reading the source failed. The previous output stays visible — a
    /// transient read error should not blank a dashboard.
    fn fail(&self, error: String) -> Resume;
}

/// What the loop does with the first good output after a failed read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Resume {
    /// Publish it even when it equals the last one, so a surface that shows
    /// the failure leaves it.
    Republish,
    /// Publish it only if it moved, as any other output.
    IfChanged,
}

/// Subscribe to the source's bus, then read and compute once, outside the
/// loop, for a mount that needs an output before it can hand anything back.
/// The receiver goes to [`spawn`]: anything that changes after the
/// subscription is queued on it, so the loop never misses a change and never
/// re-reads what this pass read.
pub(crate) async fn compute_once<A>(
    name: &str,
    source: &Dio,
    aggregation: &A,
) -> (broadcast::Receiver<DioEvent>, Result<A::Output>)
where
    A: Aggregation,
    A::Output: DebugRowCount,
{
    let bus = source.subscribe_events();
    let tap = source.inner.tap();
    let start = tap.enabled().then(Instant::now);
    let output = computed(source, aggregation)
        .await
        .map(|(rows_in, output)| {
            tap_recompute(tap, name, start, rows_in, &output, "initial", "published");
            output
        });
    (bus, output)
}

/// Spawn the loop on the source's Lens runtime. The loop holds the source and
/// the publisher; the mount's owner stops it, and releases both, by dropping
/// the returned guard.
///
/// `bus` and `already_published` come from [`compute_once`]. With an output
/// already published the loop waits for the next change; without one (the
/// first read failed) it reads the source at once.
pub(crate) fn spawn<A, P>(
    name: &str,
    source: &Dio,
    aggregation: Arc<A>,
    publisher: Arc<P>,
    (bus, nudge): (broadcast::Receiver<DioEvent>, Arc<Notify>),
    already_published: Option<A::Output>,
) -> AbortOnDrop
where
    A: Aggregation,
    A::Output: DebugRowCount,
    P: Publish<A::Output>,
{
    let lens = &source.inner.lens;
    let task = Task {
        name: name.to_string(),
        source: source.clone(),
        aggregation,
        publisher,
        last: already_published,
    };
    AbortOnDrop(
        lens.runtime
            .spawn(task.run(bus, nudge, lens.tuning().aggregate_debounce)),
    )
}

struct Task<A: Aggregation, P> {
    name: String,
    source: Dio,
    aggregation: Arc<A>,
    publisher: Arc<P>,
    last: Option<A::Output>,
}

impl<A, P> Task<A, P>
where
    A: Aggregation,
    A::Output: DebugRowCount,
    P: Publish<A::Output>,
{
    async fn run(
        mut self,
        mut bus: broadcast::Receiver<DioEvent>,
        nudge: Arc<Notify>,
        debounce: Duration,
    ) {
        // The tap is the source dio's, not this layer's own — an aggregate has
        // no datasource of its own to opt in, so its debug lines are inherited
        // from whatever the source was built with.
        let tap = self.source.inner.tap().clone();

        // A first read that failed left nothing published: retry it now
        // rather than waiting for the source to change.
        if self.last.is_none() {
            self.recompute(&tap, "initial").await;
        }

        loop {
            // Idle until something happens.
            let trigger = match wait(&mut bus, &nudge, &tap).await {
                Wake::Changed(trigger) => trigger,
                Wake::SourceGone => return,
            };

            // Leading edge: react at once, so a single change feels immediate.
            self.recompute(&tap, trigger.as_deref().unwrap_or("")).await;

            // Cooldown: absorb the burst, then flush once. The trailing flush is
            // not optional — dropping whatever arrived during the cooldown would
            // strand the layer on a stale value with nothing left to wake it.
            let deadline = tokio::time::Instant::now() + debounce;
            let mut dirty = false;
            loop {
                match tokio::time::timeout_at(deadline, wait(&mut bus, &nudge, &tap)).await {
                    Ok(Wake::Changed(_)) => dirty = true,
                    Ok(Wake::SourceGone) => return,
                    Err(_) => break,
                }
            }
            if dirty {
                self.recompute(&tap, "debounce-trailing").await;
            }
        }
    }

    /// Read, compute, and publish only if the output moved.
    async fn recompute(&mut self, tap: &DebugTap, trigger: &str) {
        let start = tap.enabled().then(Instant::now);
        let (rows_in, output) = match computed(&self.source, self.aggregation.as_ref()).await {
            Ok(computed) => computed,
            Err(e) => {
                if self.publisher.fail(e.to_string()) == Resume::Republish {
                    self.last = None;
                }
                return;
            }
        };

        // The comparison is what makes unconditional recomputation affordable:
        // recomputing often is fine as long as it does not repaint often.
        let unchanged = self.last.as_ref() == Some(&output);
        let verdict = if unchanged { "unchanged" } else { "published" };
        tap_recompute(tap, &self.name, start, rows_in, &output, trigger, verdict);
        if unchanged {
            return;
        }

        self.publisher.publish(output.clone()).await;
        self.last = Some(output);
    }
}

/// The number of rows read and the aggregation's output over them. A
/// [`Reads::Count`](crate::aggregate::Reads::Count) over an unnarrowed Dio is
/// answered from the cache's row count, without reading a row.
async fn computed<A: Aggregation>(source: &Dio, aggregation: &A) -> Result<(usize, A::Output)> {
    let reads = aggregation.reads();
    if let Some(count) = cached_count(source, &reads).await?
        && let Some(output) = A::Output::from_count(count)
    {
        return Ok((count as usize, output));
    }
    let rows = admitted_rows(source, &reads).await?;
    let output = aggregation.compute(&rows, &source.inner.metadata);
    Ok((rows.len(), output))
}

/// The debug line for one recomputation. Everything it formats stays behind
/// the gate, so it costs nothing when the tap is off.
fn tap_recompute<O: DebugRowCount>(
    tap: &DebugTap,
    name: &str,
    start: Option<Instant>,
    rows_in: usize,
    output: &O,
    trigger: &str,
    verdict: &str,
) {
    if !tap.enabled() {
        return;
    }
    let ms = start.map(|s| s.elapsed().as_millis()).unwrap_or(0);
    tapline!(
        tap,
        "derive",
        "\"{}\" recomputed from {} rows → {} in {} ({}, {})",
        name,
        num(rows_in),
        num(output.debug_row_count()),
        dur(ms as u64),
        trigger,
        verdict,
    );
}
