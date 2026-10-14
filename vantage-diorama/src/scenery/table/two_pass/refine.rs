//! Publishing a two-pass view's visible map: in the index's own order, or
//! refined (filtered and sorted) over the cache.

use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

use crate::dio::DioInner;
use crate::dio::query_index::QueryIndex;
use crate::lens::CacheStatus;

use super::super::row_map::RowMap;
use super::super::state::SharedRows;
use super::{cached, enriched, list, variant_key};

/// Publish the visible map for an **unrefined** view: the index's ids in the
/// index's own order, each row read from the cache and stamped
/// `Fresh`/`Incomplete` per its status. Built detached and swapped in one
/// write, so the map never passes through a partial state a concurrent
/// `row`/`row_count` could observe.
async fn publish_index_order(state: &Arc<SharedRows>, dio_inner: &Arc<DioInner>, ids: &[String]) {
    let mut rows = BTreeMap::new();
    let mut id_to_idx = HashMap::new();
    for (i, id) in ids.iter().enumerate() {
        let Some((rec, status)) = cached(state, dio_inner, id).await else {
            continue;
        };
        rows.insert(i, Arc::new(enriched(rec, status)));
        id_to_idx.insert(id.clone(), i);
    }
    // An unrefined view judges nothing, so it leaves no row out.
    state
        .omits_failed_rows
        .store(false, std::sync::atomic::Ordering::SeqCst);
    install(state, rows, id_to_idx, true);
}

/// Swap in a rebuilt visible map, unless the view failed its query
/// meanwhile: a failed view shows no rows.
///
/// A map that answers the whole query (`answers`) settles the view, so one
/// coming back from a failure stops reading as loading.
fn install(
    state: &SharedRows,
    rows: BTreeMap<usize, Arc<crate::scenery::enriched_record::EnrichedRecord>>,
    id_to_idx: HashMap<String, usize>,
    answers: bool,
) {
    if state.is_failed() {
        return;
    }
    *state.rows.write().unwrap() = RowMap::from_map(rows);
    *state.id_to_idx.write().unwrap() = id_to_idx;
    if answers {
        state.mark_settled("two-pass map published");
    }
}

/// Replace the visible map from `index`, in one swap. A refined view — one
/// carrying a filter, a search or an order — is rebuilt by
/// [`reseed_filtered`], the only code that applies those over the cache; an
/// unrefined view presents the index's own order.
pub(super) async fn publish(
    state: &Arc<SharedRows>,
    dio_inner: &Arc<DioInner>,
    index: &QueryIndex,
) {
    if state.local_refine() {
        reseed_filtered(state).await;
    } else {
        publish_index_order(state, dio_inner, &index.ids()).await;
    }
}

/// Rebuild the visible map for a **locally-refined** two-pass scenery: take the
/// index's ids, read each row's current cache record + status, keep those the
/// view query and the Dio's residual admit, sort by the view's order, and
/// renumber. The row's `Fresh`/`Incomplete` status is preserved.
///
/// The map settles the view only when it answers the whole query:
/// - the list pass has listed the index to the end;
/// - when the query or the Dio's residual names an augmented value, no
///   listed row still waits for its detail pass. Until then such a row is
///   left out rather than judged on a gap, where `Eq` would drop it and
///   `Ne` would admit it, and matches surface as rows hydrate.
///
/// A row whose detail fetch failed is never judged either. It stops the wait
/// but is left out, and while any listed row is left out so the view reads
/// `Partial`, not `Complete`: the set it shows is short by those rows.
///
/// A map that doesn't answer the query takes the answer back: the view reads
/// as loading.
pub(crate) async fn reseed_filtered(state: &Arc<SharedRows>) {
    use super::super::plan::admitted_by_view;

    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    let _serial = state.reseed_lock.lock().await;
    let Some(index) = state.index() else {
        return;
    };
    let ids = index.ids();
    let query = state.view_query();
    // A titles-only picker never hydrates: a clause on an augmented column
    // fails it (see `Serving::decide`), and its search runs over the titles
    // it lists.
    let judges_augmented = !state.titles_only && dio_inner.names_augmented(&query);

    let mut gathered = Vec::with_capacity(ids.len());
    let mut statuses: HashMap<String, CacheStatus> = HashMap::with_capacity(ids.len());
    let mut awaiting = Vec::new();
    let mut failed = 0usize;
    for id in &ids {
        let Some((rec, status)) = cached(state, &dio_inner, id).await else {
            continue;
        };
        if judges_augmented && dio_inner.detail_failed(id) {
            failed += 1;
            continue;
        }
        if judges_augmented && dio_inner.awaits_hydration(id, &rec, status) {
            awaiting.push(id.clone());
            continue;
        }
        if admitted_by_view(&dio_inner, &query, &rec) {
            statuses.insert(id.clone(), status);
            gathered.push((id.clone(), rec));
        }
    }
    query.sort_rows(&mut gathered, &dio_inner.metadata);

    let mut rows = BTreeMap::new();
    let mut id_to_idx = HashMap::new();
    for (i, (id, rec)) in gathered.into_iter().enumerate() {
        let status = statuses.remove(&id).unwrap_or(CacheStatus::Incomplete);
        rows.insert(i, Arc::new(enriched(rec, status)));
        id_to_idx.insert(id, i);
    }
    let answers = index.is_complete() && awaiting.is_empty();
    if !answers {
        state.unsettle();
    }
    state
        .omits_failed_rows
        .store(failed > 0, std::sync::atomic::Ordering::SeqCst);
    install(state, rows, id_to_idx, answers);
    if index.is_complete() && !awaiting.is_empty() {
        heal_sweep(state, awaiting);
    }
}

/// Queue the rows a complete index still waits on, when this view's sweep
/// has nothing queued: a write or a list page put a row back without its
/// augmented values after the sweep passed it, and the sweep's memo would
/// skip it for good. The fetch re-checks the row rather than the memo, and a
/// row it settles empty or fails on stops waiting, so this can't loop.
fn heal_sweep(state: &SharedRows, awaiting: Vec<String>) {
    let Some(ticket) = state.sweep_ticket.as_ref() else {
        return;
    };
    if ticket.is_idle() {
        ticket.enqueue(awaiting);
    }
}

/// Soft-refresh a two-pass scenery after its `(conditions, order)` changed.
///
/// Re-points the scenery at the ordered index for the new variant, presents
/// that order from cache **immediately** (responsive — the grid never blanks),
/// and re-issues the last viewport so the detail pass hydrates the
/// newly-visible rows in the background (eventually precise). Without this, a
/// sort change left the stale index in place and never restarted hydration —
/// augmentation appeared to "stop" until the user happened to scroll.
pub(crate) async fn resort(state: Arc<SharedRows>) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };

    // 1. Re-point at the index for the new variant. Reusing a variant opened
    //    earlier finds its index already built.
    let new_index = dio_inner.query_index(&variant_key(&state, &dio_inner));
    state.set_index(Some(new_index.clone()));

    // 2. A sort variant we've never listed needs one list page to learn its
    //    order. A variant we've seen before reorders straight from cache — no
    //    fetch, no blank. A refined view needs its whole index, so one whose
    //    index is still partial lists the rest (and reads as loading).
    let partial = state.local_refine() && !new_index.is_complete();
    if partial {
        state.unsettle();
    }
    if new_index.is_empty() || partial {
        list::run_list_page(state.clone()).await;
    }

    // 3. Rebuild the sparse map in one atomic swap, so the new order replaces
    //    the old in a single write — the grid never blanks mid-reorder.
    publish(&state, &dio_inner, &new_index).await;
    // Ids enqueued for the previous order stay queued — the scheduler's
    // settled recheck makes any that no longer need hydration free no-ops.
    state.bump_generation();

    // 4. Restart the detail pass for the last viewport so augmentation resumes
    //    without waiting for the user to scroll. Background: this re-drives
    //    per-row hydration, which fills rows already on screen in outline.
    super::detail::resume_hydration(&state);
}

/// Update one row's slot from its current cache state — the two-pass
/// response to `RecordChanged`: the row's values moved (a facade read
/// hydrated it, an event handler patched it) but the set's membership and
/// order did not, so touching the index — let alone re-listing — would be
/// wasted work. Ids the index doesn't hold are ignored; membership changes
/// arrive as their own events and re-derive the index.
pub(crate) async fn update_row_from_cache(state: &Arc<SharedRows>, id: &str) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    // A locally-refined view's membership follows the row's VALUES — a
    // freshly-hydrated row may start (or stop) matching the filter — so the
    // visible-map lookup below can't gate it: a matching row that wasn't
    // visible yet has no slot. Re-derive the whole visible set whenever the
    // id belongs to this view's candidate index at all, once per batch of
    // such rows (see `coalesce`).
    if state.local_refine() {
        if state.index().is_some_and(|ix| ix.contains(id)) {
            state.coalescer.mark_dirty();
        }
        return;
    }
    let Some(i) = state.id_to_idx.read().unwrap().get(id).copied() else {
        return;
    };
    let Some((rec, status)) = cached(state, &dio_inner, id).await else {
        return;
    };
    state
        .rows
        .write()
        .unwrap()
        .insert(i, Arc::new(enriched(rec, status)));
    state.bump_generation();
}
