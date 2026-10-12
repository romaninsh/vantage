//! A burst of pushed changes applied in as few cache write transactions as
//! possible.
//!
//! A durable cache commits every write transaction to disk, so applying a
//! burst one change at a time costs a commit per row. The watch task instead
//! takes every change the stream already has ready and writes each run of
//! upserts in one transaction. A run stays well under the event bus's
//! capacity, and the task yields after each one, so subscribers keep up
//! instead of lagging.

use ciborium::Value as CborValue;
use futures::{FutureExt, StreamExt};
use indexmap::IndexMap;
use vantage_core::{Result, VantageError};
use vantage_types::Record;
use vantage_vista::{VistaChange, VistaChangeStream};

use crate::dio::{Dio, DioEvent};
use crate::lens::make_dio::EVENT_BUS_CAPACITY;
use crate::ops::ChangeEvent;

/// Most changes one batch takes, so a stream that never runs dry still lets
/// its rows reach the view.
const MAX_BATCH: usize = 1024;

/// Most upserts one cache transaction takes: half the event bus, so the
/// run's events fit with room to spare.
const MAX_RUN: usize = EVENT_BUS_CAPACITY / 2;

/// Most milliseconds a run waits for room on the bus once yielding hasn't
/// made any.
const ROOM_WAIT_MS: usize = 10;

/// What the stream yielded after the batch's last change, ending it early.
pub(super) enum Tail {
    Failed(VantageError),
    Ended,
}

/// `first`, then every change `stream` has ready without waiting, up to
/// [`MAX_BATCH`].
pub(super) fn ready_batch(
    first: VistaChange,
    stream: &mut VistaChangeStream,
) -> (Vec<ChangeEvent>, Option<Tail>) {
    let mut batch = vec![first.into()];
    while batch.len() < MAX_BATCH {
        match stream.next().now_or_never() {
            None => break,
            Some(Some(Ok(change))) => batch.push(change.into()),
            Some(Some(Err(e))) => return (batch, Some(Tail::Failed(e))),
            Some(None) => return (batch, Some(Tail::Ended)),
        }
    }
    (batch, None)
}

impl Dio {
    /// Apply `changes` in order, leaving the cache and the bus in the state
    /// [`apply_change`](Self::apply_change) one at a time would. Within a run
    /// of inserts and updates carrying their row, only the last value per id
    /// is written, and the run's events follow its commit.
    ///
    /// A change that fails doesn't stop the ones after it; the first error is
    /// returned once every change has been tried.
    pub(super) async fn apply_changes(&self, changes: Vec<ChangeEvent>) -> Result<()> {
        let mut run = Run::default();
        let mut first_error = None;
        for change in changes {
            let applied = match change {
                ChangeEvent::Inserted {
                    id,
                    new: Some(record),
                } => {
                    let event = |id| DioEvent::RecordInserted { id };
                    run.push(self, id, record, event).await
                }
                ChangeEvent::Updated {
                    id,
                    new: Some(record),
                } => {
                    let event = |id| DioEvent::RecordChanged { id };
                    run.push(self, id, record, event).await
                }
                other => {
                    let flushed = run.flush(self).await;
                    let applied = self.apply_change(other).await;
                    flushed.and(applied)
                }
            };
            if let Err(e) = applied {
                first_error.get_or_insert(e);
            }
        }
        if let Err(e) = run.flush(self).await {
            first_error.get_or_insert(e);
        }
        first_error.map_or(Ok(()), Err)
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

    /// Commit the run in one transaction, then send its events. When the
    /// transaction fails, each row is written on its own and only the rows
    /// that landed get their events; the first failure is returned.
    async fn flush(&mut self, dio: &Dio) -> Result<()> {
        if self.events.is_empty() {
            return Ok(());
        }
        let rows = std::mem::take(&mut self.rows);
        let events = std::mem::take(&mut self.events);
        let cache = &dio.inner.cache;
        let mut failed = Vec::new();
        let mut first_error = None;
        if let Err(e) = cache.insert_values(rows.clone()).await {
            tracing::warn!(error = %e, "watch: batched cache write failed; writing rows one by one");
            for (id, record) in &rows {
                if let Err(e) = cache.insert_value(id, record).await {
                    failed.push(id.clone());
                    first_error.get_or_insert(e);
                }
            }
        }
        let bus = &dio.inner.event_bus;
        wait_for_room(bus, events.len()).await;
        for (id, event) in events {
            if !failed.contains(&id) {
                let _ = bus.send(event(id));
            }
        }
        tokio::task::yield_now().await;
        first_error.map_or(Ok(()), Err)
    }
}

/// Wait briefly until `bus` has room for `n` more events, so subscribers
/// that are keeping up read the run without lagging. The wait is bounded: a
/// subscriber still busy after it lags, which every bus reader handles.
async fn wait_for_room(bus: &tokio::sync::broadcast::Sender<DioEvent>, n: usize) {
    let has_room = || bus.len() + n <= EVENT_BUS_CAPACITY;
    for _ in 0..MAX_RUN {
        if has_room() {
            return;
        }
        tokio::task::yield_now().await;
    }
    for _ in 0..ROOM_WAIT_MS {
        if has_room() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
}

#[cfg(test)]
mod tests;
