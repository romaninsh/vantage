//! Pushed changes applied in as few cache write transactions as possible.
//!
//! A durable cache commits every write transaction to disk, so applying a
//! burst one change at a time costs a commit per row. The follow instead
//! takes every change the stream already has ready and writes each run of up
//! to [`MAX_RUN`] upserts in one transaction. A run's events go out after its
//! commit, and the task yields after each run, so subscribers that keep up
//! read every event. One that still falls behind lags, and every reader of
//! the bus recovers from a lag by re-reading.

use std::sync::Weak;

use ciborium::Value as CborValue;
use futures::{FutureExt, StreamExt};
use indexmap::IndexMap;
use vantage_core::{Result, VantageError};
use vantage_types::Record;
use vantage_vista::{VistaChange, VistaChangeStream};

use super::admit::Admit;
use crate::dio::{Dio, DioEvent, DioInner};
use crate::lens::spawn::EVENT_BUS_CAPACITY;
use crate::ops::ChangeEvent;

/// Most upserts one cache transaction takes: half the event bus, so the
/// run's events fit with room to spare.
const MAX_RUN: usize = EVENT_BUS_CAPACITY / 2;

/// Why a pass ended before the stream ran out of ready changes.
pub(super) enum Tail {
    /// The stream yielded an error.
    Failed(VantageError),
    /// The stream ended.
    Ended,
    /// The Dio dropped mid-pass; what was left is abandoned.
    Dropped,
}

/// Apply `first`, then every change `stream` has ready without waiting, in
/// order, each judged by `admit`, to the Dio `dio` points at. Leaves the
/// cache and the bus as [`Dio::apply_change`] one change at a time would:
/// within a run, only the last value per id is written.
///
/// Changes `admit` can't judge cost one re-read of the set, once the ready
/// changes are in, or after at most `MAX_RUN` more changes on a stream that
/// stays ready. A change that fails doesn't stop the ones after it; the
/// first error is returned once every change has been tried. The pass holds
/// the Dio only while it applies a change, so a stream that never runs dry
/// doesn't keep a dropped Dio alive.
pub(super) async fn apply_ready(
    dio: &Weak<DioInner>,
    first: VistaChange,
    stream: &mut VistaChangeStream,
    admit: &Admit,
) -> (Result<()>, Option<Tail>) {
    let upgrade = || dio.upgrade().map(|inner| Dio { inner });
    let mut run = Run::default();
    let mut errors = FirstError::default();
    let mut reread = false;
    let mut since_reread = 0;
    let mut next = Some(first);
    let tail = loop {
        let change = match next.take() {
            Some(change) => change,
            None => match stream.next().now_or_never() {
                None => break None,
                Some(Some(Ok(change))) => change,
                Some(Some(Err(e))) => break Some(Tail::Failed(e)),
                Some(None) => break Some(Tail::Ended),
            },
        };
        let Some(dio) = upgrade() else {
            return (Ok(()), Some(Tail::Dropped));
        };
        match admit.judge(change.into()) {
            Some(change) => errors.keep(run.apply(&dio, change).await),
            None => reread = true,
        }
        // A stream that stays ready doesn't hold a pending re-read back:
        // every `MAX_RUN` changes it runs, and the task yields.
        since_reread += 1;
        if reread && since_reread >= MAX_RUN {
            errors.keep(run.flush(&dio).await);
            errors.keep(dio.apply_change(ChangeEvent::Invalidated).await);
            (reread, since_reread) = (false, 0);
            tokio::task::yield_now().await;
        }
    };
    let Some(dio) = upgrade() else {
        return (Ok(()), Some(Tail::Dropped));
    };
    errors.keep(run.flush(&dio).await);
    if reread {
        errors.keep(dio.apply_change(ChangeEvent::Invalidated).await);
    }
    (errors.into_result(), tail)
}

/// The first of several errors.
#[derive(Default)]
struct FirstError(Option<VantageError>);

impl FirstError {
    fn keep(&mut self, result: Result<()>) {
        if let Err(e) = result {
            self.0.get_or_insert(e);
        }
    }

    fn into_result(self) -> Result<()> {
        self.0.map_or(Ok(()), Err)
    }
}

/// Builds the event announcing one id's change.
type EventFor = fn(String) -> DioEvent;

/// Upserts waiting for one commit, and the events to send once it lands.
#[derive(Default)]
struct Run {
    rows: IndexMap<String, Record<CborValue>>,
    events: Vec<(String, EventFor)>,
}

impl Run {
    /// Apply `change`: an insert or update carrying its row joins the run;
    /// anything else flushes the run, then applies on its own.
    async fn apply(&mut self, dio: &Dio, change: ChangeEvent) -> Result<()> {
        match change {
            ChangeEvent::Inserted {
                id,
                new: Some(record),
            } => {
                self.push(dio, id, record, |id| DioEvent::RecordInserted { id })
                    .await
            }
            ChangeEvent::Updated {
                id,
                new: Some(record),
            } => {
                self.push(dio, id, record, |id| DioEvent::RecordChanged { id })
                    .await
            }
            other => {
                let flushed = self.flush(dio).await;
                let applied = dio.apply_change(other).await;
                flushed.and(applied)
            }
        }
    }

    /// Queue an upsert; a later row for the same id replaces the earlier one.
    /// A full run is flushed.
    async fn push(
        &mut self,
        dio: &Dio,
        id: String,
        record: Record<CborValue>,
        event: EventFor,
    ) -> Result<()> {
        self.rows.insert(id.clone(), record);
        self.events.push((id, event));
        if self.events.len() >= MAX_RUN {
            self.flush(dio).await
        } else {
            Ok(())
        }
    }

    /// Commit the run in one transaction, then send its events and yield.
    /// When the transaction fails, each row is written on its own and only
    /// the rows that landed get their events; the first failure is returned.
    async fn flush(&mut self, dio: &Dio) -> Result<()> {
        if self.events.is_empty() {
            return Ok(());
        }
        let rows = std::mem::take(&mut self.rows);
        let events = std::mem::take(&mut self.events);
        let cache = &dio.inner.cache;
        let mut failed = Vec::new();
        let mut errors = FirstError::default();
        if let Err(e) = cache.insert_values(rows.clone()).await {
            tracing::warn!(error = %e, "watch: batched cache write failed; writing rows one by one");
            for (id, record) in &rows {
                if let Err(e) = cache.insert_value(id, record).await {
                    failed.push(id.clone());
                    errors.keep(Err(e));
                }
            }
        }
        for (id, event) in events {
            if !failed.contains(&id) {
                let _ = dio.inner.event_bus.send(event(id));
            }
        }
        tokio::task::yield_now().await;
        errors.into_result()
    }
}

#[cfg(test)]
mod tests;
