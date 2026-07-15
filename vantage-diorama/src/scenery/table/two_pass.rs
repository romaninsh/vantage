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

/// Whether this scenery's conditions or sort touch a column produced by
/// augmentation — the case that needs full-index hydration before the predicate
/// can be evaluated. A native (list-pass) column is already present on the cheap
/// cached rows, so it needs none.
fn references_augmented_column(
    state: &Arc<TableSceneryState>,
    dio_inner: &Arc<crate::dio::DioInner>,
) -> bool {
    let cond_aug = state
        .conditions
        .read()
        .unwrap()
        .iter()
        .any(|(col, _)| dio_inner.is_augmented_column(col));
    let sort_aug = state
        .sort
        .read()
        .unwrap()
        .as_ref()
        .is_some_and(|(col, _)| dio_inner.is_augmented_column(col));
    cond_aug || sort_aug
}

/// Rebuild the visible map for a **locally-refined** two-pass scenery: take the
/// index's ids, read each row's current cache record + status, keep those that
/// match the conditions/search, sort locally if requested, and renumber. An
/// augmented-column condition naturally excludes rows not yet hydrated (the
/// column is absent → no match), so matches surface as rows hydrate. The row's
/// `Fresh`/`Incomplete` status is preserved.
pub(crate) async fn reseed_filtered(state: &Arc<TableSceneryState>) {
    use super::helpers::{cbor_cmp, matches_conditions, matches_search, record_get_path};

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
            && matches_conditions(&rec, &conditions)
            && matches_search(&rec, search.as_deref())
        {
            gathered.push((id.clone(), rec, status));
        }
    }

    if let Some((col, dir)) = &sort {
        gathered.sort_by(|(_, a, _), (_, b, _)| {
            let ord = cbor_cmp(record_get_path(a, col), record_get_path(b, col));
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
    // The list pass comes from an explicitly-registered Lens `on_list_page`
    // callback when there is one — a deliberate override (e.g. serving list
    // pages from an `on_start`-warmed cache instead of re-walking the
    // master). Otherwise the Dio's own augmentation provides the generic
    // capability-aware pass. Bail if neither is available.
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

    let result = list_page_into(&state, &dio_inner, &index).await;

    *state.list_in_flight.lock().unwrap() = false;

    match result {
        Ok((base, new_ids)) => {
            seed_rows(&state, &dio_inner, base, &new_ids).await;
            state.bump_generation();
            let _ = dio_inner.event_bus.send(DioEvent::RangeLoaded {
                range: base..index.len(),
            });
        }
        Err(e) => {
            let end = index.len();
            let _ = dio_inner.event_bus.send(DioEvent::LoadFailed {
                range: end..end,
                error: e.to_string(),
            });
        }
    }
}

/// Fetch one list page at `index`'s tail, stub cache statuses for the
/// returned rows, and append their ids to `index`. Returns where the novel
/// ids landed and which they were (`(base, appended)` — the index skips ids
/// it already holds). Shared by [`run_list_page`] (live index — the caller
/// also seeds the sparse map) and [`refresh_index`] (detached index —
/// swapped in whole once built). Touches neither `state.index` nor the
/// sparse map.
async fn list_page_into(
    state: &Arc<TableSceneryState>,
    dio_inner: &Arc<crate::dio::DioInner>,
    index: &Arc<crate::dio::query_index::QueryIndex>,
) -> vantage_core::Result<(usize, Vec<String>)> {
    let limit = state.page_size.max(1);
    let offset = index.len();
    let q = descriptor(state, offset, limit);
    let dio = Dio {
        inner: dio_inner.clone(),
    };
    let rows = if let Some(cb) = dio_inner.lens.callbacks.on_list_page.as_ref() {
        cb(&dio, q).await?
    } else {
        crate::dio::augment_passes::list_page(&dio, q).await?
    };

    // The gap rule: a dio-augmented row whose list values already
    // fill every augment column (a file's own `size`) has nothing to
    // fetch — stub it `Complete` so the detail pass skips it.
    // Hand-rolled two-pass (`on_load_detail` lens callbacks) keeps
    // the always-`Incomplete` stubbing: it declares no augment
    // columns, so every row is the detail pass's business.
    let augmented = if dio_inner.has_dio_augment() {
        dio_inner.augmented_columns.read().unwrap().clone()
    } else {
        std::collections::HashSet::new()
    };
    let gap_aware = dio_inner.has_dio_augment() && !augmented.is_empty();
    let mut new_ids = Vec::with_capacity(rows.len());
    for (id, rec) in &rows {
        match dio_inner
            .cache
            .get_value_with_status(id)
            .await
            .ok()
            .flatten()
        {
            // Never demote a record the detail pass already completed.
            Some((_, CacheStatus::Complete)) => {}
            // Existing incomplete row: overlay the fresh list fields,
            // but augment-owned values already merged into the cache
            // win — a deliberately-demoted row (stale-while-refetch)
            // must keep displaying its old value, not blank it.
            Some((prev, CacheStatus::Incomplete)) => {
                let mut merged = prev;
                for (k, v) in rec {
                    if augmented.contains(k) && merged.get(k).is_some() {
                        continue;
                    }
                    merged.insert(k.clone(), v.clone());
                }
                let _ = dio_inner
                    .cache
                    .insert_value_with_status(id, &merged, CacheStatus::Incomplete)
                    .await;
            }
            None => {
                let status = if gap_aware
                    && !crate::dio::augment_passes::has_augment_gap(rec, &augmented)
                {
                    CacheStatus::Complete
                } else {
                    CacheStatus::Incomplete
                };
                let _ = dio_inner
                    .cache
                    .insert_value_with_status(id, rec, status)
                    .await;
            }
        }
        new_ids.push(id.clone());
    }
    Ok(index.append_page(new_ids, limit))
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
    // Ids enqueued for the previous order stay queued — the scheduler's
    // settled recheck makes any that no longer need hydration free no-ops.
    state.bump_generation();

    // 4. Restart the detail pass for the last viewport so augmentation resumes
    //    without waiting for the user to scroll.
    state.refresh_loaded_viewport();
}

/// Update one row's slot from its current cache state — the two-pass
/// response to `RecordChanged`: the row's values moved (a facade read
/// hydrated it, an event handler patched it) but the set's membership and
/// order did not, so touching the index — let alone re-listing — would be
/// wasted work. Ids the index doesn't hold are ignored; membership changes
/// arrive as their own events and re-derive the index.
pub(crate) async fn update_row_from_cache(state: &Arc<TableSceneryState>, id: &str) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    // A locally-refined view's membership follows the row's VALUES — a
    // freshly-hydrated row may start (or stop) matching the filter — so the
    // visible-map lookup below can't gate it: a matching row that wasn't
    // visible yet has no slot. Re-derive the whole visible set whenever the
    // id belongs to this view's candidate index at all.
    if state.local_refine {
        if !state.index().map(|ix| ix.contains(id)).unwrap_or(false) {
            return;
        }
        reseed_filtered(state).await;
        state.bump_generation();
        return;
    }
    let Some(i) = state.id_to_idx.read().unwrap().get(id).copied() else {
        return;
    };
    let Some((rec, status)) = dio_inner
        .cache
        .get_value_with_status(id)
        .await
        .ok()
        .flatten()
    else {
        return;
    };
    let enriched = match status {
        CacheStatus::Complete => EnrichedRecord::fresh(rec),
        CacheStatus::Incomplete => EnrichedRecord::incomplete(rec),
    };
    state.rows.write().unwrap().insert(i, Arc::new(enriched));
    state.bump_generation();
}

/// Re-derive the scenery's index from a fresh list pass — the two-pass
/// analogue of a cache reseed. The cache can gain or lose rows behind a
/// built index (a background sync pump appending pages, the refresh pass
/// reconciling against the master), and an index is append-only and possibly
/// already complete — so a refresh REPLACES the shared index for this
/// variant, re-lists, and rebuilds the sparse map in one swap (same shape as
/// [`resort`]).
pub(crate) async fn refresh_index(state: &Arc<TableSceneryState>) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };

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
    if !dio_inner.has_dio_augment() && dio_inner.lens.callbacks.on_list_page.is_none() {
        return;
    }

    // Single-flight, shared with the regular list pass. A concurrent page
    // load wins; the next event retries the refresh.
    {
        let mut guard = state.list_in_flight.lock().unwrap();
        if *guard {
            return;
        }
        *guard = true;
    }

    // Build the replacement spine OFF TO THE SIDE: the scenery keeps serving
    // its current index and map while pages land here, so a refresh never
    // passes through an empty row set (which would blank the grid and reset
    // a UI's cursor). Re-list as deep as the user had scrolled — at least as
    // many rows as the old index held.
    let target = state.index().map(|i| i.len()).unwrap_or(0);
    let fresh = Arc::new(crate::dio::query_index::QueryIndex::new());
    loop {
        if let Err(e) = list_page_into(state, &dio_inner, &fresh).await {
            *state.list_in_flight.lock().unwrap() = false;
            let _ = dio_inner.event_bus.send(DioEvent::LoadFailed {
                range: fresh.len()..fresh.len(),
                error: e.to_string(),
            });
            return;
        }
        if fresh.is_complete() || fresh.len() >= target {
            break;
        }
    }
    *state.list_in_flight.lock().unwrap() = false;

    // Read the rows for the new spine before touching anything visible; each
    // shows `Fresh`/`Incomplete` per its cached status.
    let ids = fresh.ids();
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

    // Swap in one motion: shared registry, scenery index, sparse map.
    dio_inner
        .query_indexes
        .lock()
        .unwrap()
        .insert(key, fresh.clone());
    state.set_index(Some(fresh));
    *state.rows.write().unwrap() = rows;
    *state.id_to_idx.write().unwrap() = id_to_idx;
    if state.local_refine {
        reseed_filtered(state).await;
    }
    state.bump_generation();

    // Resume hydration for what's on screen without waiting for a scroll.
    state.refresh_loaded_viewport();
}

/// Run the detail pass for `range`: collect each indexed id in the range that
/// is not already settled and hand the batch to the Dio's central augment
/// scheduler. The fetches themselves run on the scheduler's workers — one
/// flight per row across every open view, round-robin between views — and
/// each hydrated row comes back as a `RecordChanged` broadcast, which the
/// reactor turns into an in-place slot update ([`update_row_from_cache`]).
/// Skips ids already hydrated, so re-entering the same viewport — or
/// switching to a variant whose rows are already `Fresh` — enqueues nothing.
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
    // The DEMAND gate: a dio-owned augment fetches only while some open
    // scenery demands an augmented column. A tree of folder names never pays
    // for folder sizes; opening the listing beside it (which demands `size`)
    // is what starts the fetches, and closing it stops them. Rows stay
    // `Incomplete` — a later demanding viewport hydrates them.
    if dio_inner.has_dio_augment() && !dio_inner.augment_demanded() {
        tracing::trace!(
            target: "vantage_diorama::augment",
            "detail pass idle — no open view demands an augment column",
        );
        return;
    }
    // A predicate/sort on an *augmented* column can't be evaluated until rows are
    // hydrated, so such a view hydrates the whole index rather than just the
    // visible window (the documented cost of filtering/sorting on a client-side
    // column). A condition/sort on a *native* (list-pass) column needs no extra
    // hydration — `reseed_filtered` already orders/filters the cheap cache rows —
    // so it keeps normal viewport-driven hydration.
    let range = if state.local_refine && references_augmented_column(&state, &dio_inner) {
        0..index.len()
    } else {
        range
    };
    // Gap-aware skip rule (enumerable dio augments only): a row is settled
    // when it is `Complete` AND its augment columns are filled. Status alone
    // can't be trusted to skip — a cache warmed by plain inserts (an
    // `on_start` sync pump) stores rows as `Complete` without ever running
    // the detail pass. The status still matters for the refresh pass's
    // stale-while-refetch demotion: an `Incomplete` row with filled (stale)
    // augment columns must refetch, which a column check alone would skip.
    let augmented = dio_inner.augmented_columns.read().unwrap().clone();
    let gap_aware = dio_inner.has_dio_augment() && !augmented.is_empty();

    let mut pending = Vec::new();
    let mut seeded = false;
    for idx in range {
        let Some(id) = index.id_at(idx) else {
            continue;
        };
        let cached = dio_inner.cache.get_value_with_status(&id).await.ok().flatten();
        // Materialize the slot if this scenery never seeded it: the index is
        // shared per query, so a stretch of it may have been listed by a
        // SIBLING scenery — whose run_list_page seeded only its own map.
        // The viewport pass reads the cache row anyway; putting it on screen
        // is free. (Locally-refined views own their map wholesale — skip.)
        if !state.local_refine
            && let Some((row, status)) = &cached
            && !state.rows.read().unwrap().contains_key(&idx)
        {
            let enriched = match status {
                CacheStatus::Complete => EnrichedRecord::fresh(row.clone()),
                CacheStatus::Incomplete => EnrichedRecord::incomplete(row.clone()),
            };
            state.rows.write().unwrap().insert(idx, Arc::new(enriched));
            state.id_to_idx.write().unwrap().insert(id.clone(), idx);
            seeded = true;
        }
        if let Some((row, CacheStatus::Complete)) = &cached {
            let no_gap = !gap_aware || !crate::dio::augment_passes::has_augment_gap(row, &augmented);
            if no_gap {
                continue;
            }
        }
        pending.push(id);
    }
    if seeded {
        state.bump_generation();
    }
    if pending.is_empty() {
        return;
    }
    if let Some(ticket) = state.augment_ticket.as_ref() {
        ticket.enqueue(pending);
    }
}

/// React to `DioEvent::RecordLoadFailed { id }`: a scheduled detail fetch for
/// a row of ours failed. Stamp the slot so the failure is visible while the
/// partial (list-pass) columns stay on screen. Locally-refined views skip the
/// stamp — their visible set is re-derived wholesale, same as the old inline
/// pass.
pub(crate) fn mark_detail_failed(state: &Arc<TableSceneryState>, id: &str, error: &str) {
    if state.local_refine {
        return;
    }
    let Some(i) = state.id_to_idx.read().unwrap().get(id).copied() else {
        return;
    };
    let prev = state.rows.read().unwrap().get(&i).cloned();
    if let Some(prev) = prev {
        let failed =
            EnrichedRecord::detail_failed(prev.record.clone(), error.to_string(), prev.fetched_at);
        state.rows.write().unwrap().insert(i, Arc::new(failed));
        state.bump_generation();
    }
}
