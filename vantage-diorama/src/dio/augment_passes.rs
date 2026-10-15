//! The two-pass augmentation passes, owned by the Dio.
//!
//! A Dio built with [`DioBuilder::augment`](crate::DioBuilder::augment) drives
//! its own list / detail / refresh passes from the augmentations it holds.
//! These free functions are the shared bodies, read off `DioInner`.
//! Augmentation is the only way to declare two-pass loading.

mod hydrate;
mod page;

use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_types::Record;

use crate::dio::Dio;
use crate::lens::CacheStatus;

pub(crate) use hydrate::{hydrate_gaps, hydrate_one};
pub(crate) use page::{ListedPage, list_page};

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
    /// Someone asked — a Refresh button, `Dio::refresh`, or a scenery's
    /// `refresh_source`.
    Requested,
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
    // A detail record that appears after a row settled empty is picked up by
    // a requested refresh only (see `detail_state`): re-probing settled-empty
    // rows on every scheduled tick never converges, each probe a
    // `RecordChanged` and a repaint.
    let scheduled = matches!(kind, RefreshKind::Scheduled);
    dio.inner.clear_detail_states(scheduled);
    if !scheduled {
        tracing::debug!(
            target: "vantage_diorama::augment",
            "explicit refresh — re-probing rows that previously had no detail record",
        );
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
