//! The list pass: cheap rows, a page at a time, into the per-query index.

use std::sync::Arc;

use crate::dio::query_index::QueryIndex;
use crate::dio::{Dio, DioEvent, DioInner};
use crate::lens::CacheStatus;

use super::super::state::TableSceneryState;
use super::{cached, enriched, refine, variant_key};

/// Seed the sparse map for `ids` starting at row index `base`, reading each
/// id's current status from the detail cache. `Complete` rows show `Fresh`,
/// everything else shows `Incomplete`. A failed view is left empty.
async fn seed_rows(
    state: &Arc<TableSceneryState>,
    dio_inner: &Arc<DioInner>,
    base: usize,
    ids: &[String],
) {
    for (i, id) in ids.iter().enumerate() {
        let Some((rec, status)) = cached(state, dio_inner, id).await else {
            continue;
        };
        if state.is_failed() {
            return;
        }
        let idx = base + i;
        state
            .rows
            .write()
            .unwrap()
            .insert(idx, Arc::new(enriched(rec, status)));
        state.id_to_idx.write().unwrap().insert(id.clone(), idx);
    }
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

/// Run the list pass: fetch `page_size` cheap rows at the current index tail,
/// write them to the detail table as `Incomplete` (unless already `Complete`),
/// append their ids to the index, and publish the visible map.
///
/// An unrefined view lists one page and seeds its new slots. A refined view
/// evaluates its query over the whole listed set, so it lists page after
/// page until the index is complete and only then publishes, in one swap;
/// it reads as loading until then, never as a complete set of first-page
/// matches.
///
/// Sequential / no-total: a page shorter than the requested limit, or one
/// reaching the total the source states, marks the index complete (see
/// [`QueryIndex::append_page`]).
pub(crate) async fn run_list_page(state: Arc<TableSceneryState>) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    let Some(index) = state.index() else {
        return;
    };
    if !dio_inner.has_dio_augment() || index.is_complete() {
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
    if state.local_refine() {
        state.unsettle();
    }
    state.publish_view_stats();

    let result = list_pages(&state, &dio_inner, &index).await;

    state.clear_list_in_flight();

    match result {
        Ok((base, new_ids)) => {
            if state.local_refine() {
                // Settles the view once its map answers the whole query,
                // which for a view refining on an augmented value means
                // once its sweep has hydrated every listed row; the sweep
                // starts here.
                refine::reseed_filtered(&state).await;
                if index.is_complete() {
                    super::detail::resume_hydration(&state);
                }
            } else {
                seed_rows(&state, &dio_inner, base, &new_ids).await;
                // The list pass is what puts rows on screen for an
                // unrefined view; the detail pass only enriches them, so
                // waiting for hydration before dropping the loading state
                // would hold it for the whole table.
                state.mark_settled("two-pass list page");
            }
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

/// List one page, then, while the view is refined (re-read each page, as a
/// setter may refine it meanwhile), further pages until the index is
/// complete. Returns where the novel ids landed and which they were.
///
/// A page that adds no id leaves an index that can't grow: a source
/// ignoring the offset repeats rows it already gave, so listing stops there
/// rather than looping.
async fn list_pages(
    state: &Arc<TableSceneryState>,
    dio_inner: &Arc<DioInner>,
    index: &Arc<QueryIndex>,
) -> vantage_core::Result<(usize, Vec<String>)> {
    let limit = page_limit(state, dio_inner);
    let (base, mut new_ids) = list_page_into(state, dio_inner, index, limit).await?;
    let mut grew = !new_ids.is_empty();
    while grew && state.local_refine() && !index.is_complete() && !state.is_failed() {
        let (_, page) = list_page_into(state, dio_inner, index, limit).await?;
        grew = !page.is_empty();
        new_ids.extend(page);
    }
    Ok((base, new_ids))
}

/// How many rows one list page asks for: the view's page size, or, for a
/// refined view over a master without `Window`, every row. Such a master
/// answers each page with a full listing it then cuts, and a refined view
/// lists to the end, so paging it would list the whole master once per
/// page; asking for the rest lists it once.
fn page_limit(state: &TableSceneryState, dio_inner: &DioInner) -> usize {
    let windows = dio_inner
        .master
        .capabilities()
        .has(vantage_vista::Capability::Window);
    if !windows && state.local_refine() {
        usize::MAX
    } else {
        state.page_size.max(1)
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
    dio_inner: &Arc<DioInner>,
    index: &Arc<QueryIndex>,
    limit: usize,
) -> vantage_core::Result<(usize, Vec<String>)> {
    let offset = index.len();
    let dio = Dio {
        inner: dio_inner.clone(),
    };
    // Only allocate a request id when the tap is enabled — the one
    // correlator that ties this page's "list page dispatch" to its "list
    // page return" in the debug stream.
    let req = state.debug_tap.enabled().then(|| dio_inner.next_req());
    crate::debug::tapline!(
        state.debug_tap,
        "dio",
        "list #{} asks for {} ids from offset {}",
        req.unwrap_or_default(),
        if limit == usize::MAX {
            "all the".to_string()
        } else {
            crate::debug::num(limit).to_string()
        },
        crate::debug::num(offset),
    );
    let t = std::time::Instant::now();
    let (rows, total) = crate::dio::augment_passes::list_page(&dio, offset, limit).await?;

    // The gap rule: a row whose list values already fill every augment
    // column (a file's own `size`) has nothing to fetch — stub it `Complete`
    // so the detail pass skips it.
    let augmented = &dio_inner.augmented_columns;
    let gap_aware = !augmented.is_empty();
    let mut new_ids = Vec::with_capacity(rows.len());
    // A row this pass writes may have hydrated between the read below and
    // the write, which then puts the list row back over the detail. The
    // sweep's memo would still call it settled and never fetch it again, so
    // each row written here leaves the memo; a settled row costs one cache
    // read on the next sweep.
    let forget = |id: &str| {
        dio_inner.augment_settled.lock().unwrap().remove(id);
    };
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
                forget(id);
            }
            None => {
                let status =
                    if gap_aware && !crate::dio::augment_passes::has_augment_gap(rec, augmented) {
                        CacheStatus::Complete
                    } else {
                        CacheStatus::Incomplete
                    };
                let _ = dio_inner
                    .cache
                    .insert_value_with_status(id, rec, status)
                    .await;
                forget(id);
            }
        }
        new_ids.push(id.clone());
    }
    let rows_len = rows.len();
    let appended = index.append_page(new_ids, limit, total);
    // Rows came back, none new, and nobody else appended meanwhile: the
    // source repeated rows from before `offset`, so it ignores the offset
    // and the index can't grow. On a windowed source, a page's worth of rows
    // inserted ahead of `offset` between two page reads looks the same and
    // completes the index early; a refresh re-lists it.
    if rows_len > 0 && appended.1.is_empty() && appended.0 == offset {
        index.mark_complete();
    }
    crate::debug::tapline!(
        state.debug_tap,
        "dio",
        "list #{} got {} ids in {} — {} known so far{}",
        req.unwrap_or_default(),
        crate::debug::num(rows_len),
        crate::debug::dur(t.elapsed().as_millis() as u64),
        crate::debug::num(index.len()),
        if index.is_complete() {
            ", that's all of them"
        } else {
            ""
        },
    );
    Ok(appended)
}

/// Re-derive the scenery's index from a fresh list pass — the two-pass
/// analogue of a cache reseed. The cache can gain or lose rows behind a
/// built index (a background sync pump appending pages, the refresh pass
/// reconciling against the master), and an index is append-only and possibly
/// already complete — so a refresh REPLACES the shared index for this
/// variant, re-lists, and rebuilds the sparse map in one swap (same shape as
/// [`resort`](super::resort)).
pub(crate) async fn refresh_index(state: &Arc<TableSceneryState>) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    if !dio_inner.has_dio_augment() {
        return;
    }
    let key = variant_key(state, &dio_inner);

    // Single-flight, shared with the regular list pass. A concurrent page
    // load wins; the next event retries the refresh.
    {
        let mut guard = state.list_in_flight.lock().unwrap();
        if *guard {
            return;
        }
        *guard = true;
    }
    state.publish_view_stats();

    // Build the replacement spine OFF TO THE SIDE: the scenery keeps serving
    // its current index and map while pages land here, so a refresh never
    // passes through an empty row set (which would blank the grid and reset
    // a UI's cursor). Re-list as deep as the user had scrolled — at least as
    // many rows as the old index held — and a refined view to the end, since
    // it evaluates its query over the whole listed set.
    let target = state.index().map(|i| i.len()).unwrap_or(0);
    let fresh = Arc::new(QueryIndex::new());
    let limit = page_limit(state, &dio_inner);
    loop {
        let appended = match list_page_into(state, &dio_inner, &fresh, limit).await {
            Ok((_, appended)) => appended,
            Err(e) => {
                state.clear_list_in_flight();
                let _ = dio_inner.event_bus.send(DioEvent::LoadFailed {
                    range: fresh.len()..fresh.len(),
                    error: e.to_string(),
                });
                return;
            }
        };
        let deep_enough = fresh.len() >= target && !state.local_refine();
        if fresh.is_complete() || deep_enough || appended.is_empty() {
            break;
        }
    }
    state.clear_list_in_flight();

    // Register the new spine first: `reseed_filtered` derives the visible set
    // from the scenery's index, so it has to be the fresh one.
    dio_inner
        .query_indexes
        .lock()
        .unwrap()
        .insert(key, fresh.clone());
    state.set_index(Some(fresh.clone()));

    // Publish EXACTLY ONE visible map, built off to the side like the spine
    // above. Writing the raw index-order map first and reseeding a moment
    // later served a refined grid's whole unfiltered candidate set for the
    // length of the reseed's per-id reads, and a repaint landing inside that
    // window showed rows the filter exists to hide.
    refine::publish(state, &dio_inner, &fresh).await;
    state.bump_generation();

    // Resume hydration for what's on screen without waiting for a scroll.
    super::detail::resume_hydration(state);
}
