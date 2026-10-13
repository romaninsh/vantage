//! The two-pass augmentation passes, owned by the Dio.
//!
//! A Dio built with [`DioBuilder::augment`](crate::DioBuilder::augment) drives
//! its own list / detail / refresh passes from the augmentations it holds.
//! These free functions are the shared bodies, read off `DioInner`.
//! Augmentation is the only way to declare two-pass loading.

mod hydrate;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;

use crate::dio::Dio;
use crate::lens::CacheStatus;

pub(crate) use hydrate::{hydrate_gaps, hydrate_one};

/// What prompted a refresh — the two differ in how much they are entitled to
/// assume about the DETAIL source.
///
/// A scheduled tick re-pulls the master on a timer; it learns nothing about
/// whether a detail source gained records, so it must not invalidate
/// conclusions already reached about it. An explicit request is the user
/// saying "re-check everything", and pays for a full re-probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RefreshKind {
    /// The periodic timer (`Refresher::every`).
    Scheduled,
    /// Someone asked — a Refresh button, `Dio::refresh`, a scenery's
    /// `request_refresh`.
    Requested,
}

/// Capability-aware **list pass**: push the `[offset, offset+limit)` window down
/// to the master when it advertises `Window`, otherwise list the whole
/// set and window it locally so sequential paging still terminates.
///
/// Returns the window's rows and the set's total when it is known: the one
/// the master states, or the whole list's length.
pub(crate) async fn list_page(
    dio: &Dio,
    offset: usize,
    limit: usize,
) -> Result<(Vec<(String, Record<CborValue>)>, Option<usize>)> {
    let master = dio.master();
    if master.capabilities().has(vantage_vista::Capability::Window) {
        let page = master.fetch_window(offset, limit).await?;
        let total = page.total.and_then(|t| usize::try_from(t).ok());
        Ok((page.rows, total))
    } else {
        let rows = master.list_values().await?;
        let total = rows.len();
        Ok((
            rows.into_iter().skip(offset).take(limit).collect(),
            Some(total),
        ))
    }
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
pub(crate) async fn refresh(dio: &Dio, kind: RefreshKind) -> Result<()> {
    let fresh = dio.master().list_values().await?;
    let cache = &dio.inner.cache;
    let existing = cache.list_values_with_status().await?;
    let augmented = &dio.inner.augmented_columns;
    // A refreshed detail source may have gained records for rows that
    // previously settled empty — let them fetch again. Only on an explicit
    // request, though.
    //
    // Doing it on the scheduled tick makes the sweep unable to converge
    // whenever absences are common: every settled-empty row is un-settled,
    // re-fetched, found empty again, and re-settled — then the next tick wipes
    // the answer and asks again. With a 2s cadence and a detail source that
    // legitimately has no record for most rows, that is a permanent background
    // sweep (measured: ~33 detail fetches/sec that never finish), and every
    // hydration writes the row back and broadcasts `RecordChanged`, so the UI
    // repaints continuously. The set exists precisely to stop that, and
    // clearing it on a timer defeated it.
    //
    // Cost of the narrower rule: a detail record that appears *after* a row
    // settled empty is not picked up by the ticker alone. That is the right
    // trade — a scheduled re-pull of the MASTER is no evidence the DETAIL
    // source changed, and an explicit refresh (which is what a user reaches
    // for when data looks stale) still re-probes everything.
    // The FILLED memo goes on every refresh, scheduled or not: the reconcile
    // below demotes changed rows to `Incomplete` precisely to make the detail
    // pass refetch them, and a memo saying "settled" would have the sweep skip
    // them forever. Costs one re-read per row on the next sweep, no fetches.
    dio.inner.augment_settled.lock().unwrap().clear();
    dio.inner.augment_failed.lock().unwrap().clear();

    if matches!(kind, RefreshKind::Requested) {
        let cleared = {
            let mut settled = dio.inner.augment_settled_empty.lock().unwrap();
            let n = settled.len();
            settled.clear();
            n
        };
        if cleared > 0 {
            tracing::debug!(
                target: "vantage_diorama::augment",
                cleared,
                "explicit refresh — re-probing rows that previously had no detail record",
            );
        }
    }

    for (id, list_row) in &fresh {
        // A row with a flash in flight keeps its staged value — this
        // snapshot may predate the write. The next refresh reconciles it.
        if dio.inner.pending_flashes.contains(id) {
            continue;
        }
        let gap = has_augment_gap(list_row, augmented);
        let status = if gap {
            CacheStatus::Incomplete
        } else {
            CacheStatus::Complete
        };
        match existing.get(id) {
            Some((old, _)) => {
                if !list_fields_changed(list_row, old, augmented) {
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
        if !fresh.contains_key(id) && !dio.inner.pending_flashes.contains(id) {
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
        .any(|column| is_unfilled(list_row.get(column)))
}

/// Absent or null-like: drivers wrap nulls (SurrealDB's NONE arrives as
/// `Tag(6, Null)`), so unwrap tags before the null test — a tagged null is
/// still an unfilled cell.
fn is_unfilled(value: Option<&CborValue>) -> bool {
    match value {
        None | Some(CborValue::Null) => true,
        Some(CborValue::Tag(_, inner)) => matches!(inner.as_ref(), CborValue::Null),
        Some(_) => false,
    }
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

    use super::{has_augment_gap, list_fields_changed};
    use ciborium::Value;
    use vantage_types::Record;

    fn rec(pairs: &[(&str, Value)]) -> Record<Value> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect()
    }

    #[test]
    fn tagged_null_counts_as_an_augment_gap() {
        // SurrealDB's NONE arrives as a CBOR tagged null — still unfilled.
        let augmented: HashSet<String> = ["found".to_string()].into_iter().collect();
        let tagged = rec(&[("found", Value::Tag(6, Box::new(Value::Null)))]);
        assert!(has_augment_gap(&tagged, &augmented));
        let bare = rec(&[("found", Value::Null)]);
        assert!(has_augment_gap(&bare, &augmented));
        let filled = rec(&[("found", Value::Bool(true))]);
        assert!(!has_augment_gap(&filled, &augmented));
        let absent = rec(&[]);
        assert!(has_augment_gap(&absent, &augmented));
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
