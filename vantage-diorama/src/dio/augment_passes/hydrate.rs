//! The detail pass for one row, and the blocking form a facade read uses.

use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_types::Record;

use super::has_augment_gap;
use crate::dio::{Dio, DioInner};
use crate::lens::CacheStatus;

/// **Detail pass**: read the cheap list-pass row from the cache, run every
/// [`Augmentation`](crate::Augmentation) against it (resolve a detail Vista,
/// fetch, merge), and return the enriched row. `None` when the cache no
/// longer holds the row: it was deleted while queued, and there is nothing
/// to hydrate.
async fn load_detail(inner: &DioInner, id: &str) -> Result<Option<Record<CborValue>>> {
    let Some(mut row) = inner.cache.get_value(id).await? else {
        return Ok(None);
    };
    let Some(augmentations) = &inner.augmentations else {
        return Ok(Some(row));
    };
    let dio_name = inner.master.name();
    for aug in augmentations.iter() {
        aug.augment_row(dio_name, &inner.id_column, &mut row)
            .await?;
    }
    Ok(Some(row))
}

/// The canonical per-row hydration body, run by the augment scheduler's
/// workers: re-check the cache (an id another requester already settled
/// costs one read and zero fetches — this is what makes cross-view dedup
/// total), fetch the detail, merge it onto the cheap list-pass row, persist
/// as `Complete`, and broadcast `RecordChanged` so every open view updates
/// its slot.
pub(crate) async fn hydrate_one(inner: &Arc<DioInner>, id: &str) -> Result<()> {
    if !inner.has_dio_augment() {
        return Ok(());
    }
    let augmented = &inner.augmented_columns;
    let gap_aware = !augmented.is_empty();
    if inner.augment_settled_empty.lock().unwrap().contains(id) {
        // Hydrated before and found no detail record — a legitimate,
        // permanent gap until the next refresh clears the set.
        return Ok(());
    }
    if let Some((row, CacheStatus::Complete)) = inner.cache.get_value_with_status(id).await?
        && (!gap_aware || !has_augment_gap(&row, augmented))
    {
        return Ok(());
    }

    let Some(detail) = load_detail(inner, id).await? else {
        return Ok(());
    };

    // Merge the detail columns onto the cheap list-pass row so the list
    // columns survive hydration, then mark the row Complete. A row deleted
    // while its detail was fetched stays deleted: writing the merge would
    // bring it back.
    let Some(mut merged) = inner.cache.get_value(id).await? else {
        tracing::debug!(
            target: "vantage_diorama::augment",
            table = %inner.master.name(),
            id,
            "row deleted while its detail was fetched; the detail is dropped",
        );
        return Ok(());
    };
    for (k, v) in detail {
        merged.insert(k, v);
    }
    inner
        .cache
        .insert_value_with_status(id, &merged, CacheStatus::Complete)
        .await?;
    inner.augment_failed.lock().unwrap().remove(id);
    // Record which way this row settled. Gapped AFTER hydration means the
    // detail source has no record to fill it (legitimate absence); filled
    // means it is done. Either way the sweep must be able to skip it without
    // reading it back — otherwise gap → hydrate → still gapped → generation
    // bump → sweep is a busy loop, and even a filled row costs a cache read
    // on every sweep to re-establish what we already know here. Both sets are
    // cleared by the refresh pass.
    if gap_aware && has_augment_gap(&merged, augmented) {
        inner
            .augment_settled_empty
            .lock()
            .unwrap()
            .insert(id.to_string());
    } else {
        inner.augment_settled.lock().unwrap().insert(id.to_string());
    }
    let _ = inner
        .event_bus
        .send(crate::DioEvent::RecordChanged { id: id.to_string() });
    Ok(())
}

/// Hydrate every row in `rows` that still has an augment gap, blocking until
/// the scheduler has settled them all, then swap the enriched rows into
/// `rows` from the cache. Before the sweep a single
/// [`DioEvent::Hydrating`](crate::DioEvent::Hydrating) carries the pending
/// count — a consumer blocking on a facade read can tell the user what's
/// coming — and every hydrated row emits `RecordChanged` for progress. Going
/// through the scheduler means a facade read racing a scenery's viewport
/// shares its fetches instead of duplicating them.
pub(crate) async fn hydrate_gaps(
    dio: &Dio,
    rows: &mut indexmap::IndexMap<String, Record<CborValue>>,
) -> Result<()> {
    if !dio.inner.has_dio_augment() {
        return Ok(());
    }
    let augmented = &dio.inner.augmented_columns;
    let pending: Vec<String> = rows
        .iter()
        .filter(|(_, row)| has_augment_gap(row, augmented))
        .map(|(id, _)| id.clone())
        .collect();
    if pending.is_empty() {
        return Ok(());
    }
    let _ = dio.inner.event_bus.send(crate::DioEvent::Hydrating {
        pending: pending.len(),
    });
    dio.inner.ensure_augment_workers();
    let ticket = dio.inner.augment_scheduler.ticket();
    ticket
        .enqueue_and_wait(pending.clone())
        .await
        .map_err(|e| vantage_core::error!("augment hydration failed", detail = e))?;
    for id in pending {
        // A row deleted while it hydrated is gone from the read too.
        match dio.inner.cache.get_value(&id).await? {
            Some(full) => {
                rows.insert(id, full);
            }
            None => {
                rows.shift_remove(&id);
            }
        }
    }
    Ok(())
}
