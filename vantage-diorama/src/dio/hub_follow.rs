//! Following the table hub: applying what the other Dios over this Dio's
//! table settled, judged against this Dio's own query.
//!
//! A fact arrives as a confirmed row, a delete, a "re-check this id" or a
//! "re-read your set". The follower turns each into what this Dio's cache
//! and bus would show had the change come from its own master: a row that
//! now belongs to the set is written and announced as
//! [`RecordInserted`](DioEvent::RecordInserted) or
//! [`RecordChanged`](DioEvent::RecordChanged), one that no longer does is
//! removed and announced as [`RecordRemoved`](DioEvent::RecordRemoved). The
//! sceneries react to those as they always do.
//!
//! Nothing applied here is published again (see [`applying`]).

use std::sync::{Arc, Weak};

use ciborium::Value as CborValue;
use indexmap::IndexSet;
use tokio::sync::broadcast::{self, error::RecvError, error::TryRecvError};
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet as _;
use vantage_types::Record;
use vantage_vista::VistaQuery;

use super::watch::admit::Admit;
use super::watch::until_dio_drops;
use super::{Dio, DioEvent, DioInner, admitted_by_dio, applying};
use crate::lens::CacheStatus;
use crate::lens::hub::{HubFact, TableChange};
use crate::ops::ChangeEvent;

/// A Dio's membership in its table hub, made but not yet followed.
///
/// The receiver is made when the Dio spawns, before its eager start copies
/// the set, and followed once the copy has landed: a fact published during
/// the copy waits in the receiver and applies over the copy, rather than
/// being lost or overwritten by it.
pub(crate) struct HubFollower {
    rx: broadcast::Receiver<HubFact>,
    serial: u64,
    admit: Admit,
    /// `Weak`, as for the live follow: the hub never keeps a Dio alive.
    dio: Weak<DioInner>,
    cancel: tokio::sync::watch::Receiver<()>,
}

impl HubFollower {
    /// Join `inner`'s hub. `None` for a Dio in no hub.
    pub(crate) fn join(inner: &Arc<DioInner>) -> Option<Self> {
        let hub = inner.hub.as_ref()?;
        Some(Self {
            rx: hub.subscribe(),
            serial: inner.serial,
            admit: Admit::for_dio(inner),
            dio: Arc::downgrade(inner),
            cancel: inner.watch_cancel.subscribe(),
        })
    }

    /// Apply the other Dios' facts until this Dio drops.
    ///
    /// Facts that are already waiting are applied together, so a burst of
    /// facts for one id costs one re-read of it. A follower that fell behind
    /// the hub lost facts it can't name, so it re-reads its set instead, as
    /// it does when applying a fact fails.
    pub(crate) async fn run(mut self) {
        loop {
            let Some(first) = until_dio_drops(&mut self.cancel, self.rx.recv()).await else {
                return;
            };
            let mut batch = Batch::default();
            match first {
                Ok(fact) => batch.push(fact, self.serial),
                Err(RecvError::Lagged(_)) => batch.lagged = true,
                Err(RecvError::Closed) => return,
            }
            loop {
                match self.rx.try_recv() {
                    Ok(fact) => batch.push(fact, self.serial),
                    Err(TryRecvError::Lagged(_)) => batch.lagged = true,
                    Err(_) => break,
                }
            }
            if batch.is_empty() {
                continue;
            }
            let Some(inner) = self.dio.upgrade() else {
                return;
            };
            let dio = Dio { inner };
            if let Err(e) = applying(batch.apply(&dio, &self.admit)).await {
                tracing::warn!(
                    target: "vantage_diorama::hub",
                    table = %dio.inner.cache_table_name,
                    error = %e,
                    "applying another Dio's change failed; re-reading the set",
                );
                if let Err(e) = applying(dio.refresh()).await {
                    tracing::error!(
                        target: "vantage_diorama::hub",
                        table = %dio.inner.cache_table_name,
                        error = %e,
                        "re-reading the set failed; the cache may be out of step until the next refresh",
                    );
                }
            }
        }
    }
}

/// The facts waiting when the follower woke, other Dios' only.
#[derive(Default)]
struct Batch {
    facts: Vec<HubFact>,
    /// Facts were lost: the set must be re-read whatever the rest says.
    lagged: bool,
}

impl Batch {
    fn push(&mut self, fact: HubFact, own: u64) {
        if fact.origin != own {
            self.facts.push(fact);
        }
    }

    fn is_empty(&self) -> bool {
        self.facts.is_empty() && !self.lagged
    }

    async fn apply(self, dio: &Dio, admit: &Admit) -> Result<()> {
        let inner = &dio.inner;
        if self.lagged {
            return dio.refresh().await;
        }
        // Ids re-read through the master once every other fact is in. The
        // master's answer is the newest there is, so deferring it past a
        // later fact for the same id loses nothing.
        let mut rechecks = IndexSet::new();
        let mut reread = false;
        for HubFact { store, change, .. } in self.facts {
            if store == inner.cache_table_name {
                // The publisher shares this Dio's cache table and query: the
                // bytes are already here, only the event is missing. A Dio
                // that judges every fact through its master still does, and
                // announces the bytes only when its re-read left them alone.
                let id = match &change {
                    TableChange::Invalidated => None,
                    TableChange::Upserted { id, .. }
                    | TableChange::Deleted { id }
                    | TableChange::Recheck { id } => Some(id.clone()),
                };
                let announced = match id {
                    Some(id) if inner.recheck_facts => inner.recheck(&id).await?,
                    _ => false,
                };
                if !announced {
                    forward(dio, change).await?;
                }
                continue;
            }
            match change {
                TableChange::Invalidated => reread = true,
                // This Dio's master is the only judge of its set.
                TableChange::Upserted { id, .. } | TableChange::Deleted { id }
                    if inner.recheck_facts =>
                {
                    rechecks.insert(id);
                }
                TableChange::Recheck { id } => {
                    rechecks.insert(id);
                }
                TableChange::Deleted { id } => {
                    drop_row(dio, &id).await?;
                }
                TableChange::Upserted {
                    id,
                    complete: false,
                    ..
                } => {
                    // The row lacks augmented values a clause may name:
                    // judging it would decide on the gap.
                    rechecks.insert(id);
                }
                TableChange::Upserted { id, row, .. } => {
                    match admit.judge(ChangeEvent::Updated {
                        id: id.clone(),
                        new: Some(row),
                    }) {
                        Some(ChangeEvent::Updated { new: Some(row), .. }) => {
                            settle_row(dio, &id, row).await?;
                        }
                        Some(_) => {
                            drop_row(dio, &id).await?;
                        }
                        // The master's own clauses can't be judged in memory.
                        None => {
                            rechecks.insert(id);
                        }
                    }
                }
            }
        }
        if reread {
            return dio.refresh().await;
        }
        for id in rechecks {
            inner.recheck(&id).await?;
        }
        Ok(())
    }
}

/// Announce a fact whose bytes this Dio's cache table already holds: a
/// delete as a removal, a whole-set change as `DatasetChanged`, and a row
/// as a change while the shared cache holds it, else as a removal. A
/// re-check carries no row, and the publisher may still hold it (a
/// notify-only `apply_change`) or have dropped it (a narrowed reconcile).
async fn forward(dio: &Dio, change: TableChange) -> Result<()> {
    let event = match change {
        TableChange::Upserted { id, .. } | TableChange::Recheck { id } => {
            if dio.inner.cache.get_value(&id).await?.is_some() {
                DioEvent::RecordChanged { id }
            } else {
                DioEvent::RecordRemoved { id }
            }
        }
        TableChange::Deleted { id } => DioEvent::RecordRemoved { id },
        TableChange::Invalidated => DioEvent::DatasetChanged,
    };
    let _ = dio.inner.event_bus.send(event);
    Ok(())
}

/// Make row `id` hold `row`, a row the table now holds and this Dio's
/// master would return, and announce it. `true` when it announced
/// anything. Nothing happens when the cached row already equals it. While a
/// flash on `id` is in flight here the flash's own settle decides the row,
/// as for a reconcile, and the row is re-checked once the flash settles. A
/// row the residual turns away is removed instead.
async fn settle_row(dio: &Dio, id: &str, row: Record<CborValue>) -> Result<bool> {
    let inner = &dio.inner;
    if inner.defer_while_pending(id) {
        return Ok(false);
    }
    let held = inner.cache.get_value_with_status(id).await?;
    let (row, status) = inner.over_held(row, held.as_ref());
    if held
        .as_ref()
        .is_some_and(|(held, held_status)| *held == row && *held_status == status)
    {
        return Ok(false);
    }
    let judged_in_memory = !inner.names_augmented(&VistaQuery::new());
    if judged_in_memory && !admitted_by_dio(inner, &row) {
        return drop_row(dio, id).await;
    }
    inner
        .cache
        .insert_value_with_status(id, &row, status)
        .await?;
    // A residual naming an augmented value can only judge the stored row,
    // hydrated here when it lacks the value.
    if !judged_in_memory
        && inner
            .read_admitted(inner.cache.as_ref(), id)
            .await?
            .is_none()
    {
        inner.cache.delete_value(id).await?;
        if held.is_some() {
            let _ = inner
                .event_bus
                .send(DioEvent::RecordRemoved { id: id.into() });
        }
        return Ok(held.is_some());
    }
    let id = id.to_string();
    let event = if held.is_some() {
        DioEvent::RecordChanged { id }
    } else {
        DioEvent::RecordInserted { id }
    };
    let _ = inner.event_bus.send(event);
    Ok(true)
}

/// Remove row `id` when the cache holds it, unless a flash on it is in
/// flight (then it is re-checked once the flash settles). `true` when it
/// removed the row.
async fn drop_row(dio: &Dio, id: &str) -> Result<bool> {
    let inner = &dio.inner;
    if inner.defer_while_pending(id) || inner.cache.get_value(id).await?.is_none() {
        return Ok(false);
    }
    dio.removed(id).await?;
    Ok(true)
}

impl DioInner {
    /// Re-read row `id` through the master and make the cache agree: a row
    /// the master returns and the residual admits is written (and announced
    /// unless it is unchanged), one it doesn't return is removed. `true`
    /// when it announced anything. A row with a flash in flight is left to
    /// the flash and re-checked once it settles. Never publishes.
    pub(crate) async fn recheck(self: &Arc<Self>, id: &str) -> Result<bool> {
        if self.defer_while_pending(id) {
            return Ok(false);
        }
        let dio = Dio {
            inner: self.clone(),
        };
        match self.master.get_value(id).await? {
            Some(row) => settle_row(&dio, id, row).await,
            None => drop_row(&dio, id).await,
        }
    }

    /// Whether a flash on `id` is in flight here. If so, a hub fact about
    /// `id` can't be applied now, since the flash would overwrite it or be
    /// overwritten. The id is noted, and [`recheck_deferred`](Self::recheck_deferred)
    /// re-reads it once the flash settles, so a peer's later write isn't lost.
    fn defer_while_pending(&self, id: &str) -> bool {
        if !self.pending_flashes.contains(id) {
            return false;
        }
        self.deferred_rechecks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id.to_string());
        true
    }

    /// Whether a hub fact about `id` was skipped while a flash on it was in
    /// flight. The flash's row may then be stale outside its own fields.
    pub(crate) fn has_deferred(&self, id: &str) -> bool {
        self.deferred_rechecks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(id)
    }

    /// A flash on `id` settled: re-check `id` if a hub fact about it was
    /// skipped meanwhile. Runs as its own task, so the write queue isn't held
    /// up, and as the application of a fact, so it publishes nothing.
    pub(crate) fn recheck_deferred(self: &Arc<Self>, id: &str) {
        let deferred = self
            .deferred_rechecks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(id);
        if !deferred {
            return;
        }
        let inner = self.clone();
        let id = id.to_string();
        self.lens.runtime.spawn(async move {
            if let Err(e) = applying(inner.recheck(&id)).await {
                tracing::warn!(
                    target: "vantage_diorama::hub",
                    table = %inner.cache_table_name,
                    id = %id,
                    error = %e,
                    "re-checking a row after its flash settled failed",
                );
            }
        });
    }

    /// `row` as this Dio stores it over `held`, the row it caches now, with
    /// the status to store it under. A row from another Dio or the master
    /// may lack the augmented values this Dio's detail pass fetched; those
    /// are kept, and a row still missing one is stored `Incomplete` so the
    /// detail pass fills it.
    fn over_held(
        &self,
        mut row: Record<CborValue>,
        held: Option<&(Record<CborValue>, CacheStatus)>,
    ) -> (Record<CborValue>, CacheStatus) {
        if !self.has_dio_augment() {
            return (row, CacheStatus::Complete);
        }
        let lift_all = self.augmented_columns.is_empty();
        if let Some((held, _)) = held {
            for (column, value) in held {
                let augmented = lift_all || self.augmented_columns.contains(column);
                if augmented && !row.contains_key(column) {
                    row.insert(column.clone(), value.clone());
                }
            }
        }
        let status = if lift_all {
            held.map_or(CacheStatus::Incomplete, |(_, status)| *status)
        } else if super::augment_passes::has_augment_gap(&row, &self.augmented_columns) {
            CacheStatus::Incomplete
        } else {
            CacheStatus::Complete
        };
        (row, status)
    }
}
