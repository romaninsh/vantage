//! Two-pass loading: a sequential **list pass** that builds the per-query
//! index from cheap rows, and a viewport-driven **detail pass** that hydrates
//! expensive columns one id at a time.
//!
//! Engaged only when the Lens registers an `on_load_detail` callback (see
//! [`TableSceneryBuilder::open`](super::builder::TableSceneryBuilder::open)).
//! In single-pass mode none of this runs.

use std::ops::Range;
use std::sync::Arc;

use vantage_vista::SortDirection;

use crate::dio::{Dio, DioEvent};
use crate::lens::CacheStatus;
use crate::ops::QueryDescriptor;
use crate::scenery::enriched_record::EnrichedRecord;

use super::SortDir;
use super::state::TableSceneryState;

/// Build a [`QueryDescriptor`] for the page `offset..offset+limit` from the
/// scenery's current conditions/sort/search.
fn descriptor(state: &TableSceneryState, offset: usize, limit: usize) -> QueryDescriptor {
    let conditions = state.conditions.read().unwrap().clone();
    let sort = state.sort.read().unwrap().clone().map(|(col, dir)| {
        let dir = match dir {
            SortDir::Asc => SortDirection::Ascending,
            SortDir::Desc => SortDirection::Descending,
        };
        (col, dir)
    });
    let search = state.search.read().unwrap().clone();
    QueryDescriptor::new()
        .with_conditions(conditions)
        .with_sort(sort)
        .with_search(search)
        .with_window(offset, limit)
}

/// Seed the sparse map for `ids` starting at row index `base`, reading each
/// id's current status from the detail cache. `Complete` rows show `Fresh`,
/// everything else shows `Incomplete`.
async fn seed_rows(
    state: &Arc<TableSceneryState>,
    dio_inner: &Arc<crate::dio::DioInner>,
    base: usize,
    ids: &[String],
) {
    for (i, id) in ids.iter().enumerate() {
        let idx = base + i;
        let entry = dio_inner
            .cache
            .get_value_with_status(id)
            .await
            .ok()
            .flatten();
        let enriched = match entry {
            Some((rec, CacheStatus::Complete)) => EnrichedRecord::fresh(rec),
            Some((rec, CacheStatus::Incomplete)) => EnrichedRecord::incomplete(rec),
            None => continue,
        };
        state.rows.write().unwrap().insert(idx, Arc::new(enriched));
        state.id_to_idx.write().unwrap().insert(id.clone(), idx);
    }
}

/// Rebuild the visible map for a **locally-refined** two-pass scenery: take the
/// index's ids, read each row's current cache record + status, keep those that
/// match the conditions/search, sort locally if requested, and renumber. An
/// augmented-column condition naturally excludes rows not yet hydrated (the
/// column is absent → no match), so matches surface as rows hydrate. The row's
/// `Fresh`/`Incomplete` status is preserved.
pub(crate) async fn reseed_filtered(state: &Arc<TableSceneryState>) {
    use super::helpers::{cbor_cmp, matches_conditions, matches_search};

    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    let Some(index) = state.index() else {
        return;
    };
    let ids = index.ids();
    let conditions = state.conditions.read().unwrap().clone();
    let search = state.search.read().unwrap().clone();
    let sort = state.sort.read().unwrap().clone();

    let mut gathered: Vec<(String, vantage_types::Record<ciborium::Value>, CacheStatus)> =
        Vec::with_capacity(ids.len());
    for id in &ids {
        if let Some((rec, status)) = dio_inner
            .cache
            .get_value_with_status(id)
            .await
            .ok()
            .flatten()
        {
            if matches_conditions(&rec, &conditions) && matches_search(&rec, search.as_deref()) {
                gathered.push((id.clone(), rec, status));
            }
        }
    }

    if let Some((col, dir)) = &sort {
        gathered.sort_by(|(_, a, _), (_, b, _)| {
            let ord = cbor_cmp(a.get(col), b.get(col));
            match dir {
                SortDir::Asc => ord,
                SortDir::Desc => ord.reverse(),
            }
        });
    }

    let mut rows = std::collections::BTreeMap::new();
    let mut id_to_idx = std::collections::HashMap::new();
    for (i, (id, rec, status)) in gathered.into_iter().enumerate() {
        let enriched = match status {
            CacheStatus::Complete => EnrichedRecord::fresh(rec),
            CacheStatus::Incomplete => EnrichedRecord::incomplete(rec),
        };
        rows.insert(i, Arc::new(enriched));
        id_to_idx.insert(id, i);
    }
    *state.rows.write().unwrap() = rows;
    *state.id_to_idx.write().unwrap() = id_to_idx;
}

/// Seed the scenery's sparse map from an already-populated shared index
/// (reused across filter switches) without issuing any list call.
pub(crate) async fn seed_from_index(state: &Arc<TableSceneryState>) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    let Some(index) = state.index() else {
        return;
    };
    let ids = index.ids();
    seed_rows(state, &dio_inner, 0, &ids).await;
}

/// Run one list page: fetch `page_size` cheap rows at the current index tail,
/// write them to the detail table as `Incomplete` (unless already `Complete`),
/// append their ids to the index, and seed the new sparse-map slots.
///
/// Sequential / no-total: a page shorter than the requested limit marks the
/// index complete (see [`QueryIndex::append_page`](crate::dio::query_index::QueryIndex::append_page)).
pub(crate) async fn run_list_page(state: Arc<TableSceneryState>) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    let Some(index) = state.index() else {
        return;
    };
    // The list pass comes from the Dio's own augmentation (preferred) or the
    // legacy Lens `on_list_page` callback. Bail if neither provides one.
    if !dio_inner.has_dio_augment() && dio_inner.lens.callbacks.on_list_page.is_none() {
        return;
    }
    if index.is_complete() {
        return;
    }

    // Single-flight: don't let overlapping load-more calls double-page.
    {
        let mut guard = state.list_in_flight.lock().unwrap();
        if *guard {
            return;
        }
        *guard = true;
    }

    let limit = state.page_size.max(1);
    let offset = index.len();
    let q = descriptor(&state, offset, limit);
    let dio = Dio {
        inner: dio_inner.clone(),
    };
    let result = if dio_inner.has_dio_augment() {
        crate::dio::augment_passes::list_page(&dio, q).await
    } else {
        let cb = dio_inner.lens.callbacks.on_list_page.as_ref().unwrap();
        cb(&dio, q).await
    };

    *state.list_in_flight.lock().unwrap() = false;

    match result {
        Ok(rows) => {
            let mut new_ids = Vec::with_capacity(rows.len());
            for (id, rec) in &rows {
                // Never demote a record the detail pass already completed.
                let already_complete = matches!(
                    dio_inner
                        .cache
                        .get_value_with_status(id)
                        .await
                        .ok()
                        .flatten(),
                    Some((_, CacheStatus::Complete))
                );
                if !already_complete {
                    let _ = dio_inner
                        .cache
                        .insert_value_with_status(id, rec, CacheStatus::Incomplete)
                        .await;
                }
                new_ids.push(id.clone());
            }
            let base = index.len();
            index.append_page(new_ids.clone(), limit);
            seed_rows(&state, &dio_inner, base, &new_ids).await;
            state.bump_generation();
            let _ = dio_inner.event_bus.send(DioEvent::RangeLoaded {
                range: base..index.len(),
            });
        }
        Err(e) => {
            let _ = dio_inner.event_bus.send(DioEvent::LoadFailed {
                range: offset..offset,
                error: e.to_string(),
            });
        }
    }
}

/// Soft-refresh a two-pass scenery after its `(conditions, sort)` changed.
///
/// Re-points the scenery at the ordered index for the new variant, presents
/// that order from cache **immediately** (responsive — the grid never blanks),
/// and re-issues the last viewport so the detail pass hydrates the
/// newly-visible rows in the background (eventually precise). Without this, a
/// sort change left the stale index in place and never restarted hydration —
/// augmentation appeared to "stop" until the user happened to scroll.
pub(crate) async fn resort(state: Arc<TableSceneryState>) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };

    // 1. Re-point at the index for the new (conditions, sort) variant. Reusing
    //    a variant opened earlier finds its index already built.
    let conditions = state.conditions.read().unwrap().clone();
    let sort = state.sort.read().unwrap().clone();
    let vista_sort = sort.as_ref().map(|(col, dir)| {
        let dir = match dir {
            SortDir::Asc => SortDirection::Ascending,
            SortDir::Desc => SortDirection::Descending,
        };
        (col.as_str(), dir)
    });
    let key = dio_inner
        .master
        .read()
        .unwrap()
        .index_key(&conditions, vista_sort);
    let new_index = dio_inner.query_index(&key);
    state.set_index(Some(new_index.clone()));

    // 2. A sort variant we've never listed needs one list page to learn its
    //    order. A variant we've seen before reorders straight from cache — no
    //    fetch, no blank.
    if new_index.is_empty() {
        run_list_page(state.clone()).await;
    }

    // 3. Rebuild the sparse map from the index in one atomic swap, so the new
    //    order replaces the old in a single write — the grid never blanks to an
    //    empty map mid-reorder. Each row shows `Fresh`/`Incomplete` per its
    //    cached status.
    let ids = new_index.ids();
    let mut rows = std::collections::BTreeMap::new();
    let mut id_to_idx = std::collections::HashMap::new();
    for (i, id) in ids.iter().enumerate() {
        let Some((rec, status)) = dio_inner
            .cache
            .get_value_with_status(id)
            .await
            .ok()
            .flatten()
        else {
            continue;
        };
        let enriched = match status {
            CacheStatus::Complete => EnrichedRecord::fresh(rec),
            CacheStatus::Incomplete => EnrichedRecord::incomplete(rec),
        };
        rows.insert(i, Arc::new(enriched));
        id_to_idx.insert(id.clone(), i);
    }
    *state.rows.write().unwrap() = rows;
    *state.id_to_idx.write().unwrap() = id_to_idx;
    // Old in-flight detail claims belonged to the previous order — drop them so
    // the restarted detail pass isn't blocked by stale single-flight guards.
    state.detail_in_flight.lock().unwrap().clear();
    state.bump_generation();

    // 4. Restart the detail pass for the last viewport so augmentation resumes
    //    without waiting for the user to scroll.
    state.refresh_loaded_viewport();
}

/// Run the detail pass for `range`: for each indexed id in the range that is
/// not already `Complete` (and not in flight), fetch its detail, merge it into
/// the cache as `Complete`, and flip the row to `Fresh`. Skips ids already
/// hydrated, so re-entering the same viewport — or switching to a variant whose
/// rows are already `Fresh` — issues zero detail calls.
pub(crate) async fn run_detail_for_range(state: Arc<TableSceneryState>, range: Range<usize>) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    let Some(index) = state.index() else {
        return;
    };
    if !dio_inner.has_dio_augment() && dio_inner.lens.callbacks.on_load_detail.is_none() {
        return;
    }
    // A locally-refined view can't know which rows match an augmented-column
    // predicate until they're hydrated, so it hydrates the whole index rather
    // than just the visible window. (Bounded by the listed set; large sets pay
    // for this — the documented cost of filtering on a client-side column.)
    let range = if state.local_refine {
        0..index.len()
    } else {
        range
    };
    let dio = Dio {
        inner: dio_inner.clone(),
    };
    let mut changed = false;

    for idx in range {
        let Some(id) = index.id_at(idx) else {
            continue;
        };

        let status = dio_inner
            .cache
            .get_value_with_status(&id)
            .await
            .ok()
            .flatten()
            .map(|(_, s)| s);
        if status == Some(CacheStatus::Complete) {
            continue;
        }

        // Claim the id; skip if another detail fetch already owns it.
        {
            let mut inflight = state.detail_in_flight.lock().unwrap();
            if !inflight.insert(id.clone()) {
                continue;
            }
        }

        let result = if dio_inner.has_dio_augment() {
            let catalog = dio_inner.augment_catalog.read().unwrap().clone();
            let augs = dio_inner.augmentations.read().unwrap().clone();
            match (catalog, augs) {
                (Some(catalog), Some(augs)) => {
                    crate::dio::augment_passes::load_detail_with(&dio, id.clone(), &catalog, &augs)
                        .await
                }
                _ => Ok(Default::default()),
            }
        } else {
            let cb = dio_inner.lens.callbacks.on_load_detail.as_ref().unwrap();
            cb(&dio, id.clone()).await
        };
        state.detail_in_flight.lock().unwrap().remove(&id);

        match result {
            Ok(detail) => {
                // Merge the detail columns onto the cheap list-pass row so the
                // list columns survive hydration, then mark the row Complete.
                let mut merged = dio_inner
                    .cache
                    .get_value(&id)
                    .await
                    .ok()
                    .flatten()
                    .unwrap_or_default();
                for (k, v) in detail {
                    merged.insert(k, v);
                }
                let _ = dio_inner
                    .cache
                    .insert_value_with_status(&id, &merged, CacheStatus::Complete)
                    .await;
                // A locally-refined view re-derives its whole visible set after
                // the loop (a freshly-hydrated row may now match/sort differently);
                // a plain view updates the row's slot in place.
                if !state.local_refine
                    && let Some(i) = state.id_to_idx.read().unwrap().get(&id).copied()
                {
                    state
                        .rows
                        .write()
                        .unwrap()
                        .insert(i, Arc::new(EnrichedRecord::fresh(merged)));
                }
                changed = true;
            }
            Err(e) => {
                if !state.local_refine
                    && let Some(i) = state.id_to_idx.read().unwrap().get(&id).copied()
                {
                    let prev = state.rows.read().unwrap().get(&i).cloned();
                    if let Some(prev) = prev {
                        let failed = EnrichedRecord::detail_failed(
                            prev.record.clone(),
                            e.to_string(),
                            prev.fetched_at,
                        );
                        state.rows.write().unwrap().insert(i, Arc::new(failed));
                    }
                }
                let _ = dio_inner.event_bus.send(DioEvent::LoadFailed {
                    range: idx..idx + 1,
                    error: e.to_string(),
                });
                changed = true;
            }
        }
    }

    if changed {
        // Re-derive the locally-refined visible set now that more rows are
        // hydrated (newly-matching rows appear, no-longer-matching drop).
        if state.local_refine {
            reseed_filtered(&state).await;
        }
        state.bump_generation();
    }
}
