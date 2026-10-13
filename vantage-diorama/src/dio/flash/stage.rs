//! The cache side of a flash: staging it before the write-through, and
//! settling it once the write-through answers.

use ciborium::Value as CborValue;
use vantage_core::{Result, error};
use vantage_types::{Record, cbor_id_to_string};

use crate::dio::{Dio, DioEvent, DioInner};
use crate::ops::{ChangeFlash, FlashKind, FlashOutcome};

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

/// Put `pre` back under `id`, but only while the row still holds `staged`,
/// the value this flash staged. A later flash on the same id that staged
/// over it owns the row now, and restoring would overwrite its value.
/// `true` when the row was restored.
pub(super) async fn restore_if_staged(
    inner: &DioInner,
    id: &str,
    pre: Option<&Record<CborValue>>,
    staged: Option<&Record<CborValue>>,
) -> Result<bool> {
    if inner.cache.get_value(id).await?.as_ref() != staged {
        return Ok(false);
    }
    write_row(inner, id, pre).await?;
    Ok(true)
}

/// Settle `flash` once its write-through answered `written`, and report
/// the outcome its caller receives.
///
/// - A staged flash commits (the row settles, re-keyed when the backend
///   moved it) or rolls back to `pre` and returns the write's error. The
///   rollback is skipped when a later flash on the same id has staged over
///   `staged` since.
/// - An id-less insert seeds the cache with the stored row under the id the
///   write reported.
/// - A `Clear` empties the cache table.
///
/// A failed id-less write has no row to revert, so it is announced as
/// [`DioEvent::WriteFailed`].
pub(crate) async fn settle(
    dio: &Dio,
    flash: &ChangeFlash,
    pre: Option<&Record<CborValue>>,
    staged: Option<&Record<CborValue>>,
    written: Result<FlashOutcome>,
) -> Result<FlashOutcome> {
    let inner = &dio.inner;
    match (flash.id(), written) {
        (Some(id), Ok(outcome)) => commit(dio, flash, id, pre, outcome).await,
        (Some(id), Err(err)) => {
            restore_if_staged(inner, id, pre, staged).await?;
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
async fn commit(
    dio: &Dio,
    flash: &ChangeFlash,
    id: &str,
    pre: Option<&Record<CborValue>>,
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
        if record.is_some() {
            let _ = inner.event_bus.send(DioEvent::RecordInserted {
                id: settled.clone(),
            });
        }
        return Ok(FlashOutcome {
            id: Some(settled),
            record,
        });
    }
    reassert_confirmed(inner, flash).await?;
    let id_owned = id.to_string();
    let event = match (flash.kind(), pre) {
        (FlashKind::Delete, _) => DioEvent::RecordRemoved { id: id_owned },
        (_, None) => DioEvent::RecordInserted { id: id_owned },
        _ => DioEvent::RecordChanged { id: id_owned },
    };
    let _ = inner.event_bus.send(event);
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

/// An id-less insert the backend confirmed: the row joins the cache under
/// the id the write reported. Once the write landed the id is returned even
/// if seeding the cache fails (logged): the row exists upstream, and a
/// caller that lost its id would insert it again on retry.
async fn seed_new_row(
    dio: &Dio,
    flash: &ChangeFlash,
    outcome: FlashOutcome,
) -> Result<FlashOutcome> {
    let inner = &dio.inner;
    let id = outcome
        .id
        .clone()
        .or_else(|| outcome.record.as_ref().and_then(|r| stored_id(dio, r)))
        .ok_or_else(|| {
            error!(
                "the write stored a new row without reporting its id",
                table = inner.cache_table_name
            )
        })?;
    let mut record = outcome.record.unwrap_or_else(|| flash.patch().clone());
    record
        .entry(dio.id_column().to_string())
        .or_insert_with(|| CborValue::Text(id.clone()));
    match inner.cache.insert_value(&id, &record).await {
        Ok(()) => {
            let _ = inner
                .event_bus
                .send(DioEvent::RecordInserted { id: id.clone() });
        }
        Err(e) => tracing::warn!(
            target: "vantage_diorama",
            id = %id,
            error = %e,
            "inserted row not seeded into the cache; it shows after the next refresh",
        ),
    }
    Ok(FlashOutcome {
        id: Some(id),
        record: Some(record),
    })
}

/// `flash`'s fields merged over `base`.
fn merged(base: Option<&Record<CborValue>>, flash: &ChangeFlash) -> Record<CborValue> {
    let mut merged = base.cloned().unwrap_or_default();
    for (k, v) in flash.patch() {
        merged.insert(k.clone(), v.clone());
    }
    merged
}

/// The id a stored record carries in the Dio's id column, read in the form
/// every cache key takes.
fn stored_id(dio: &Dio, record: &Record<CborValue>) -> Option<String> {
    record.get(dio.id_column()).and_then(cbor_id_to_string)
}
