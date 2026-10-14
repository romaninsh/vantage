//! The detail pass: queue the visible rows that still need hydrating, and
//! show the ones whose hydration failed.
//!
//! - `reactions` — restarting hydration after a reorder, and stamping a
//!   failed fetch.

mod reactions;

use std::ops::Range;
use std::sync::Arc;

use crate::dio::augment_scheduler::AugmentTicket;
use crate::dio::{DetailState, DioInner};
use crate::lens::CacheStatus;

use super::super::state::SharedRows;
use super::{cached, enriched};

pub(crate) use reactions::{mark_detail_failed, resume_hydration};

/// Whether this view hydrates its whole listed index rather than the rows on
/// screen: it is refined, and its query or the Dio's residual names an
/// augmented value (see [`DioInner::names_augmented`]). Such a view can't
/// tell which rows belong on screen until every listed row is hydrated.
/// A titles-only picker never sweeps: it never hydrates.
pub(crate) fn sweeps_whole_index(state: &SharedRows, dio_inner: &DioInner) -> bool {
    !state.titles_only && state.local_refine() && dio_inner.names_augmented(&state.view_query())
}

/// Run the detail pass for `range` (positions on screen): collect each id
/// to hydrate that is not already settled and hand the batch to the Dio's
/// central augment scheduler. The fetches themselves run on the scheduler's
/// workers — one flight per row across every open view, round-robin between
/// views — and each hydrated row comes back as a `RecordChanged` broadcast,
/// which the reactor turns into an in-place slot update
/// ([`update_row_from_cache`](super::update_row_from_cache)). Skips ids already
/// hydrated, so re-entering the same viewport — or switching to a variant
/// whose rows are already `Fresh` — enqueues nothing.
///
/// The ids are:
/// - for a view that [sweeps](sweeps_whole_index), every listed id;
/// - for another refined view, the ids its displayed map shows in `range`
///   (it shows its own order, not the index's);
/// - for an unrefined view, the index's ids in `range`.
///
/// The batch is queued under `ticket`: the asking view's, or the rows' sweep
/// ticket.
pub(crate) async fn run_detail_for_range(
    state: Arc<SharedRows>,
    range: Range<usize>,
    ticket: Option<Arc<AugmentTicket>>,
) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    let Some(index) = state.index() else {
        return;
    };
    if !dio_inner.has_dio_augment() {
        return;
    }
    // Hoisted: `local_refine` reads five locks to answer, and the query cannot
    // change under a sweep that never yields to a mutator.
    let local_refine = state.local_refine();
    let sweep = sweeps_whole_index(&state, &dio_inner);
    // The DEMAND gate: an augment fetches only while some open scenery
    // demands an augmented column. A tree of folder names never pays for
    // folder sizes; opening the listing beside it (which demands `size`) is
    // what starts the fetches, and closing it stops them. Rows stay
    // `Incomplete` — a later demanding viewport hydrates them. A sweeping
    // view demands by its predicate, whatever columns it shows.
    if !sweep && !dio_inner.augment_demanded() {
        tracing::trace!(
            target: "vantage_diorama::augment",
            "detail pass idle — no open view demands an augment column",
        );
        return;
    }
    // What the user is actually looking at. Kept even when the sweep widens
    // below, because it decides the ORDER work is queued in.
    let visible = range.clone();
    let targets: Vec<(usize, String)> = if sweep {
        index.ids().into_iter().enumerate().collect()
    } else if local_refine {
        let displayed = state.id_to_idx.read().unwrap();
        let mut on_screen: Vec<(usize, String)> = displayed
            .iter()
            .filter(|(_, position)| visible.contains(position))
            .map(|(id, position)| (*position, id.clone()))
            .collect();
        on_screen.sort();
        on_screen
    } else {
        range
            .filter_map(|idx| index.id_at(idx).map(|id| (idx, id)))
            .collect()
    };
    // Gap-aware skip rule (enumerable augments only): a row is settled when
    // it is `Complete` AND its augment columns are filled. Status alone can't
    // be trusted to skip — a cache warmed by plain inserts (an eager start)
    // stores rows as `Complete` without ever running the detail pass. The
    // status still matters for the refresh pass's stale-while-refetch
    // demotion: an `Incomplete` row with filled (stale) augment columns must
    // refetch, which a column check alone would skip.
    let augmented = &dio_inner.augmented_columns;
    let gap_aware = !augmented.is_empty();

    // `(idx, id)` rather than bare ids, so the queue can be ordered by where a
    // row sits relative to the viewport.
    let mut pending: Vec<(usize, String)> = Vec::new();
    let mut seeded = false;
    // Sweep census, for the log line below. A detail pass that keeps enqueueing
    // the same rows every time it runs is the signature of a hydration loop,
    // and without these counts it is indistinguishable from ordinary scrolling.
    let mut settled_empty_skips = 0usize;
    let mut already_complete = 0usize;
    let mut memo_skips = 0usize;
    let requested = targets.len();
    for (idx, id) in targets {
        // Memo fast path. Establishing "settled" from the record costs a cache
        // read, and this loop runs over the WHOLE index on a view whose
        // predicate touches an augmented column — re-derived on every
        // generation bump, so every batch of hydrations pays for the entire
        // table again. `Filled` and `Empty` are decided answers; consult them
        // before reading.
        //
        // Gated on the slot already existing for an unrefined view: that view
        // still has to materialize slots a SIBLING scenery's list pass seeded
        // into its own map only, and that genuinely needs the record. A
        // refined view owns its map wholesale and never seeds here.
        let memo = dio_inner.detail_state(&id);
        if (local_refine || state.rows.read().unwrap().contains_key(&idx))
            && matches!(memo, Some(DetailState::Filled | DetailState::Empty))
        {
            memo_skips += 1;
            continue;
        }
        let entry = cached(&state, &dio_inner, &id).await;
        // Materialize the slot if this scenery never seeded it: the index is
        // shared per query, so a stretch of it may have been listed by a
        // SIBLING scenery — whose run_list_page seeded only its own map.
        // The viewport pass reads the cache row anyway; putting it on screen
        // is free. (Locally-refined views own their map wholesale — skip.)
        if !local_refine
            && let Some((row, status)) = &entry
            && !state.rows.read().unwrap().contains_key(&idx)
        {
            state
                .rows
                .write()
                .unwrap()
                .insert(idx, Arc::new(enriched(row.clone(), *status)));
            state.id_to_idx.write().unwrap().insert(id.clone(), idx);
            seeded = true;
        }
        if let Some((row, CacheStatus::Complete)) = &entry {
            let no_gap = !gap_aware || !crate::dio::augment_passes::has_augment_gap(row, augmented);
            if no_gap {
                already_complete += 1;
                // Remember the answer we just paid a read for. This is the
                // path that populates the memo for a cache warmed before this
                // process started — `hydrate_one` only ever runs for rows the
                // sweep enqueues, so it alone would leave a fully-hydrated
                // persisted cache re-reading every row on every sweep.
                dio_inner.set_detail_state(&id, DetailState::Filled);
                continue;
            }
            // Hydrated but legitimately empty (no detail record exists) — don't
            // re-fetch until an EXPLICIT refresh forgets it (the scheduled
            // timer deliberately keeps it; see `augment_passes::RefreshKind`).
            if memo == Some(DetailState::Empty) {
                settled_empty_skips += 1;
                continue;
            }
        }
        pending.push((idx, id));
    }
    if seeded {
        state.bump_generation();
    }
    // On-screen rows first.
    //
    // A view whose filter or sort touches an augmented column has to hydrate
    // the whole table before it can decide what belongs on screen at all — but
    // that is an argument about *how many* rows, not about which order to fetch
    // them in. Queued as swept, a table of 872 rows hydrates whatever sits
    // first in the list-pass order and reaches the visible window whenever it
    // gets there, so the rows under the cursor fill in last. The cost is
    // identical either way; only the wait the user sees changes.
    //
    // "On screen" is a position in the DISPLAYED map, not in the shared index.
    // The two are the same order only for an unrefined view: a locally sorted
    // or filtered one rebuilds `rows` in its own order (`reseed_filtered`),
    // while the sweep above walks the index. Ranking by the index position
    // would prioritise whichever rows happen to occupy those slots in the
    // unsorted order — a different set, near enough at random.
    //
    // Stable within each group, so the visible window hydrates top-down and the
    // remainder keeps the sweep's order.
    {
        let displayed = state.id_to_idx.read().unwrap();
        pending.sort_by_key(|(idx, id)| {
            let position = displayed.get(id).copied().unwrap_or(*idx);
            !visible.contains(&position)
        });
    }
    let pending: Vec<String> = pending.into_iter().map(|(_, id)| id).collect();
    tracing::debug!(
        target: "vantage_diorama::augment",
        requested,
        pending = pending.len(),
        already_complete,
        settled_empty_skips,
        memo_skips,
        "detail pass census",
    );
    crate::debug::tapline!(
        state.debug_tap,
        "hydrate",
        "{} rows in view — {} need detail, {} already complete",
        crate::debug::num(requested),
        crate::debug::num(pending.len()),
        crate::debug::num(already_complete),
    );
    if pending.is_empty() {
        return;
    }
    if let Some(ticket) = ticket {
        ticket.enqueue(pending);
    }
}
