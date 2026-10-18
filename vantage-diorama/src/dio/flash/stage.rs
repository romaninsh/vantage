//! The cache side of a flash: staging it before the write-through, and
//! settling it once the write-through answers.

mod seed;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;

use super::confirmed::{confirm, roll_back};
use crate::dio::pending::PendingFlashGuard;
use crate::dio::{Dio, DioEvent, DioInner};
use crate::lens::hub::TableChange;
use crate::ops::{ChangeFlash, FlashKind, FlashOutcome};
use seed::{merged, seed_new_row, stored_id};

/// The row `flash` stages in the cache, `None` for one it removes. `Patch`
/// merges onto `base` (the cached row, or the master's when the cache
/// missed) so untouched columns survive: the cache stores whole rows.
pub(super) fn staged_value(
    flash: &ChangeFlash,
    base: Option<&Record<CborValue>>,
) -> Option<Record<CborValue>> {
    match flash.kind() {
        FlashKind::Insert | FlashKind::Replace | FlashKind::Upsert => Some(flash.patch().clone()),
        FlashKind::Patch => Some(merged(base, flash)),
        FlashKind::Delete | FlashKind::Clear => None,
    }
}

/// Make the cache row `id` hold `row`, or remove it for `None`.
pub(super) async fn write_row(
    inner: &DioInner,
    id: &str,
    row: Option<&Record<CborValue>>,
) -> Result<()> {
    match row {
        Some(row) => inner.cache.insert_value(id, row).await,
        None => inner.cache.delete_value(id).await,
    }
}

/// Settle `flash` once its write-through answered `written`, and report
/// the outcome its caller receives.
///
/// - A staged flash commits (the row settles, re-keyed when the backend
///   moved it) or rolls back and returns the write's error. A rollback
///   restores the last confirmed row, never an earlier flash's unconfirmed
///   value, and is skipped when a later flash on the same id has staged over
///   `staged` since (see [`roll_back`]).
/// - An id-less insert seeds the cache with the stored row under the id the
///   write reported.
/// - A `Clear` empties the cache table.
///
/// A failed id-less write has no row to revert, so it is announced as
/// [`DioEvent::WriteFailed`].
///
/// What a confirmed flash settles is published to the other Dios over the
/// table (see [`crate::lens::hub`]). A rollback that restored a row asks
/// them to re-check it: the restored row is never published as it is,
/// since it may be older than what another Dio holds.
pub(crate) async fn settle(
    dio: &Dio,
    flash: &ChangeFlash,
    pending: Option<&PendingFlashGuard>,
    pre: Option<&Record<CborValue>>,
    staged: Option<&Record<CborValue>>,
    written: Result<FlashOutcome>,
) -> Result<FlashOutcome> {
    let inner = &dio.inner;
    match (flash.id(), written) {
        (Some(id), Ok(outcome)) => commit(dio, flash, id, pending, pre, staged, outcome).await,
        (Some(id), Err(err)) => {
            if roll_back(inner, id, pending, pre, staged).await? {
                // The restored row may be an earlier flash's confirmed value
                // that was never published, because this flash had staged
                // over it. The others re-read the row rather than take the
                // restored one on trust: it may be older than theirs.
                inner.publish(TableChange::Recheck { id: id.to_string() });
            }
            let _ = inner.event_bus.send(DioEvent::WriteReverted {
                id: id.to_string(),
                error: err.to_string(),
                kind: *flash.kind(),
            });
            Err(err)
        }
        (None, Ok(outcome)) if *flash.kind() == FlashKind::Insert => {
            seed_new_row(dio, flash, outcome).await
        }
        (None, Ok(outcome)) => {
            inner.cache.clear().await?;
            let _ = inner.event_bus.send(DioEvent::DatasetChanged);
            // Every row of this Dio's set went: each other Dio over the
            // table re-reads its own.
            inner.publish(TableChange::Invalidated);
            Ok(outcome)
        }
        (None, Err(err)) => {
            let _ = inner.event_bus.send(DioEvent::WriteFailed {
                id: None,
                error: err.to_string(),
            });
            Err(err)
        }
    }
}

/// A confirmed staged flash. A backend may keep a new record under an id of
/// its own (a fully-qualified `table:key`, a sequence number), reported as
/// the outcome's id or as the stored record's id column. The staged row then
/// moves onto that id, because one record upstream must stay one row here.
///
/// The settle event follows membership, judged against `pre` (the cached row
/// before staging): a delete is [`DioEvent::RecordRemoved`], a row new to the
/// cache is [`DioEvent::RecordInserted`] (even if the master held it), and any
/// other write is [`DioEvent::RecordChanged`].
///
/// The flash's value becomes the id's confirmed row. Its fields are
/// re-asserted in the cache only while no later flash on the id is staged:
/// that flash's value is what the row shows until it settles.
async fn commit(
    dio: &Dio,
    flash: &ChangeFlash,
    id: &str,
    pending: Option<&PendingFlashGuard>,
    pre: Option<&Record<CborValue>>,
    staged: Option<&Record<CborValue>>,
    outcome: FlashOutcome,
) -> Result<FlashOutcome> {
    let inner = &dio.inner;
    let settled = outcome
        .id
        .clone()
        .or_else(|| outcome.record.as_ref().and_then(|r| stored_id(dio, r)))
        .filter(|settled| settled != id && *flash.kind() != FlashKind::Delete);
    if let Some(settled) = settled {
        let record = match outcome.record {
            Some(record) => Some(record),
            None => inner.cache.get_value(id).await?.map(|mut staged| {
                staged.insert(
                    dio.id_column().to_string(),
                    CborValue::Text(settled.clone()),
                );
                staged
            }),
        };
        match &record {
            Some(record) => inner.cache.insert_value(&settled, record).await?,
            // The staged row went while the write ran (a delete or a reread
            // got there first), and the backend reported no row. Seeding the
            // new id with nothing would invent a row; the next read of the
            // set brings it in.
            None => tracing::warn!(
                target: "vantage_diorama::flash",
                table = %inner.cache_table_name,
                id,
                settled = %settled,
                "the re-keyed row isn't cached: its staged value is gone",
            ),
        }
        inner.cache.delete_value(id).await?;
        let _ = inner
            .event_bus
            .send(DioEvent::RecordRemoved { id: id.to_string() });
        // The other Dios over the table never saw the minted id, but one
        // may have read it from the master meanwhile.
        inner.publish(TableChange::Deleted { id: id.to_string() });
        if record.is_some() {
            let _ = inner.event_bus.send(DioEvent::RecordInserted {
                id: settled.clone(),
            });
            inner.publish_row(&settled).await;
        }
        return Ok(FlashOutcome {
            id: Some(settled),
            record,
        });
    }
    // With a later flash on the id staged over this one, the cache shows
    // that flash's unconfirmed value: nothing is published until it settles.
    // Its commit publishes the row then, and its rollback, which restores
    // this flash's value, asks the others to re-check it.
    let confirmed = confirm(pending, flash, staged);
    if confirmed {
        reassert_confirmed(inner, flash).await?;
    }
    let id_owned = id.to_string();
    let event = match (flash.kind(), pre) {
        (FlashKind::Delete, _) => DioEvent::RecordRemoved { id: id_owned },
        (_, None) => DioEvent::RecordInserted { id: id_owned },
        _ => DioEvent::RecordChanged { id: id_owned },
    };
    let _ = inner.event_bus.send(event);
    match flash.kind() {
        // A confirmed delete removed the row from the table, whatever set
        // this Dio narrows it to.
        FlashKind::Delete => inner.publish(TableChange::Deleted { id: id.to_string() }),
        // Another Dio changed the row while this flash was in flight, and
        // that change was skipped here: the cached row is current only in
        // this flash's own fields. The others re-read it instead.
        _ if confirmed && inner.has_deferred(id) => {
            inner.publish(TableChange::Recheck { id: id.to_string() })
        }
        _ if confirmed => inner.publish_row(id).await,
        _ => {}
    }
    Ok(FlashOutcome {
        id: Some(id.to_string()),
        record: outcome.record,
    })
}

/// After the write-through confirms, make the cache agree with the
/// confirmed fields. The staged value normally still stands, but a cache
/// writer that ignored the pending guard may have replaced the row
/// mid-flight with a pre-write snapshot. Merging the flash's own fields onto
/// the *current* row re-asserts exactly what was confirmed, keeping any
/// fresher values that arrived for other fields.
async fn reassert_confirmed(inner: &DioInner, flash: &ChangeFlash) -> Result<()> {
    let Some(id) = flash.id() else {
        return Ok(());
    };
    match flash.kind() {
        FlashKind::Delete => inner.cache.delete_value(id).await,
        FlashKind::Clear => Ok(()),
        _ => {
            let current = inner.cache.get_value(id).await?;
            inner
                .cache
                .insert_value(id, &merged(current.as_ref(), flash))
                .await
        }
    }
}
