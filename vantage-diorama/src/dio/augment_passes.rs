//! The two-pass augmentation passes, owned by the Dio.
//!
//! A Dio configured with [`Dio::augment`](crate::Dio::augment) drives its own
//! list / detail / refresh passes from the augmentation config it holds. These
//! free functions are the shared bodies, read off `DioInner` — distinct from a
//! Lens that registers explicit `on_list_page`/`on_load_detail` callbacks for
//! hand-rolled two-pass.

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;
use vantage_vista_factory::VistaCatalog;

use crate::augment::Augmentation;
use crate::dio::Dio;
use crate::lens::CacheStatus;
use crate::ops::QueryDescriptor;

/// Capability-aware **list pass**: push the `[offset, offset+limit)` window down
/// to the master when it advertises `can_fetch_window`, otherwise list the whole
/// set and window it locally so sequential paging still terminates.
pub(crate) async fn list_page(
    dio: &Dio,
    desc: QueryDescriptor,
) -> Result<Vec<(String, Record<CborValue>)>> {
    let master = dio.master();
    if master.capabilities().can_fetch_window {
        master.fetch_window(desc.offset, desc.limit).await
    } else {
        let rows = master.list_values().await?;
        Ok(rows
            .into_iter()
            .skip(desc.offset)
            .take(desc.limit)
            .collect())
    }
}

/// **Detail pass**: read the cheap list-pass row from the cache, run every
/// [`Augmentation`] against it (resolve a detail Vista, fetch, merge), return the
/// enriched row.
pub(crate) async fn load_detail_with(
    dio: &Dio,
    id: String,
    catalog: &VistaCatalog,
    augmentations: &[Augmentation],
) -> Result<Record<CborValue>> {
    let mut row = dio.cache().get_value(&id).await?.unwrap_or_default();
    let master_id_column = dio.master().get_id_column().unwrap_or("id").to_string();
    let dio_name = dio.master().name().to_string();
    for aug in augmentations {
        aug.augment_row(&dio_name, &master_id_column, &mut row, catalog)
            .await?;
    }
    Ok(row)
}

/// The canonical per-row hydration body, run by the augment scheduler's
/// workers: re-check the cache (an id another requester already settled
/// costs one read and zero fetches — this is what makes cross-view dedup
/// total), fetch the detail, merge it onto the cheap list-pass row, persist
/// as `Complete`, and broadcast `RecordChanged` so every open view updates
/// its slot.
pub(crate) async fn hydrate_one(inner: &std::sync::Arc<super::DioInner>, id: &str) -> Result<()> {
    let augmented = inner.augmented_columns.read().unwrap().clone();
    let gap_aware = inner.has_dio_augment() && !augmented.is_empty();
    if let Some((row, CacheStatus::Complete)) =
        inner.cache.get_value_with_status(id).await.ok().flatten()
        && (!gap_aware || !has_augment_gap(&row, &augmented))
    {
        return Ok(());
    }

    let dio = Dio {
        inner: inner.clone(),
    };
    let detail = if inner.has_dio_augment() {
        let catalog = inner.augment_catalog.read().unwrap().clone();
        let augmentations = inner.augmentations.read().unwrap().clone();
        match (catalog, augmentations) {
            (Some(catalog), Some(augmentations)) => {
                load_detail_with(&dio, id.to_string(), &catalog, &augmentations).await?
            }
            _ => Default::default(),
        }
    } else if let Some(cb) = inner.lens.callbacks.on_load_detail.as_ref() {
        cb(&dio, id.to_string()).await?
    } else {
        return Ok(());
    };

    // Merge the detail columns onto the cheap list-pass row so the list
    // columns survive hydration, then mark the row Complete.
    let mut merged = inner
        .cache
        .get_value(id)
        .await
        .ok()
        .flatten()
        .unwrap_or_default();
    for (k, v) in detail {
        merged.insert(k, v);
    }
    inner
        .cache
        .insert_value_with_status(id, &merged, CacheStatus::Complete)
        .await?;
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
    // Two-pass covers both declarative augmentation and a hand-rolled
    // `on_load_detail` lens callback — facade reads hydrate through either.
    if !dio.inner.is_two_pass() {
        return Ok(());
    }
    let augmented = dio.inner.augmented_columns.read().unwrap().clone();
    let pending: Vec<String> = rows
        .iter()
        .filter(|(_, row)| has_augment_gap(row, &augmented))
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
        if let Some(full) = dio.inner.cache.get_value(&id).await? {
            rows.insert(id, full);
        }
    }
    Ok(())
}

/// **Refresh pass**: re-run the cheap list pass and reconcile against the cache
/// without discarding still-valid augmentation. Unchanged list fields keep their
/// hydrated detail columns + `Complete` status; changed ones merge the fresh
/// list values and demote per the gap rule; new ids stub per the gap rule;
/// vanished ids are deleted.
///
/// Augment-owned columns are excluded from the change COMPARISON — the cached
/// row's augmented value against the list's unfilled column is not a change
/// (comparing it demoted every hydrated row on every refresh: the 0 → value → 0
/// flap). A row that genuinely moved takes the fresh list values but KEEPS its
/// augmented values while the refetch is in flight — stale-while-refetch. The
/// staleness lives out-of-band in [`CacheStatus`]: `Incomplete` is what drives
/// the detail pass, so the cell never has to blank to signal the gap. Blank is
/// reserved for rows that were never filled.
pub(crate) async fn refresh(dio: &Dio) -> Result<()> {
    let fresh = dio.master().list_values().await?;
    let cache = dio.cache();
    let existing = cache.list_values_with_status().await?;
    let augmented = dio.inner.augmented_columns.read().unwrap().clone();

    for (id, list_row) in &fresh {
        let gap = has_augment_gap(list_row, &augmented);
        let status = if gap {
            CacheStatus::Incomplete
        } else {
            CacheStatus::Complete
        };
        match existing.get(id) {
            Some((old, _)) => {
                if !list_fields_changed(list_row, old, &augmented) {
                    continue;
                }
                tracing::debug!(
                    target: "vantage_diorama::augment",
                    id = %id,
                    gap,
                    "list fields moved — row demoted for augment refetch",
                );
                // Merge fresh list values over the cached row: every list
                // value (a file's own size included) wins, but augment columns
                // the list leaves unfilled KEEP their previous values — the
                // display stays on the stale number while `Incomplete` status
                // sends the row back through the detail pass.
                let mut merged = old.clone();
                for (k, v) in list_row {
                    if augmented.contains(k)
                        && matches!(v, CborValue::Null)
                        && merged.get(k).is_some()
                    {
                        continue; // list's null placeholder never erases a fill
                    }
                    merged.insert(k.clone(), v.clone());
                }
                cache.insert_value_with_status(id, &merged, status).await?;
            }
            None => {
                cache.insert_value_with_status(id, list_row, status).await?;
            }
        }
    }
    for id in existing.keys() {
        if !fresh.contains_key(id) {
            cache.delete_value(id).await?;
        }
    }
    Ok(())
}

/// The gap rule: an augmentation exists to FILL columns the list leaves
/// unfilled. A row whose list-supplied values already cover every augment
/// column (a file's own `size`) has no gap and never fetches; a row with an
/// absent/null augment column (a folder) does. An un-enumerable augment
/// (empty merge list = "lift all") always counts as a gap — there is nothing
/// to test against.
pub(crate) fn has_augment_gap(
    list_row: &Record<CborValue>,
    augmented: &std::collections::HashSet<String>,
) -> bool {
    if augmented.is_empty() {
        return true;
    }
    augmented
        .iter()
        .any(|column| matches!(list_row.get(column), None | Some(CborValue::Null)))
}

/// True when any field the list pass paints differs from the cached record.
/// Only keys present in `list_row` are compared — the detail pass's own
/// columns (which exist only in `cached`) are never inspected — and columns
/// in `augmented` are skipped even when the list supplies them: the augment
/// owns those values, so the list's cheap placeholder is never a change.
pub(crate) fn list_fields_changed(
    list_row: &Record<CborValue>,
    cached: &Record<CborValue>,
    augmented: &std::collections::HashSet<String>,
) -> bool {
    list_row
        .iter()
        .any(|(k, v)| !augmented.contains(k) && cached.get(k) != Some(v))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::list_fields_changed;
    use ciborium::Value;
    use vantage_types::Record;

    fn rec(pairs: &[(&str, Value)]) -> Record<Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    fn none() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn unchanged_list_fields_ignore_augmented_columns() {
        let list = rec(&[
            ("status", Value::from("LIVE")),
            ("updated_at", Value::from("t1")),
        ]);
        let cached = rec(&[
            ("status", Value::from("LIVE")),
            ("updated_at", Value::from("t1")),
            ("subject_area", Value::from("finance")),
        ]);
        assert!(!list_fields_changed(&list, &cached, &none()));
    }

    #[test]
    fn a_changed_non_augmented_field_is_detected() {
        let list = rec(&[
            ("status", Value::from("DECOMMISSIONED")),
            ("updated_at", Value::from("t2")),
        ]);
        let cached = rec(&[
            ("status", Value::from("LIVE")),
            ("updated_at", Value::from("t1")),
            ("subject_area", Value::from("finance")),
        ]);
        assert!(list_fields_changed(&list, &cached, &none()));
    }

    #[test]
    fn a_new_list_key_absent_from_cache_counts_as_changed() {
        let list = rec(&[
            ("status", Value::from("LIVE")),
            ("region", Value::from("eu")),
        ]);
        let cached = rec(&[("status", Value::from("LIVE"))]);
        assert!(list_fields_changed(&list, &cached, &none()));
    }

    /// The flap regression in miniature: the list paints an augment-owned
    /// column with a cheap placeholder (a folder's `size: 0`); the cached row
    /// holds the augmented value. That difference is NOT a change — the
    /// augment owns the column.
    #[test]
    fn a_list_placeholder_under_an_augmented_column_is_not_a_change() {
        let list = rec(&[("modified", Value::from("t1")), ("size", Value::from("0"))]);
        let cached = rec(&[
            ("modified", Value::from("t1")),
            ("size", Value::from("4096")),
            ("file_count", Value::from("3")),
        ]);
        let augmented: HashSet<String> = ["size", "file_count"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert!(!list_fields_changed(&list, &cached, &augmented));
        // …while a real list-field move on the same row still demotes it.
        let moved = rec(&[("modified", Value::from("t2")), ("size", Value::from("0"))]);
        assert!(list_fields_changed(&moved, &cached, &augmented));
    }
}
