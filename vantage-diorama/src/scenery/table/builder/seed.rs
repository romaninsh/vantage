//! Seeding freshly built rows at open, before their tasks start.

mod paged;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use vantage_core::Result;

use crate::dio::DioInner;
use crate::scenery::table::plan::Serving;
use crate::scenery::table::state::SharedRows;
use crate::scenery::table::two_pass;

use paged::{announce, seed_paged};

/// Say what the source can do, then put the first rows in place the way the
/// rows' shape needs: from the shared two-pass index, from a warm cache, or
/// not at all until the first load lands.
pub(super) async fn seed(state: &Arc<SharedRows>, dio: &Arc<DioInner>) -> Result<()> {
    announce(state);
    if state.two_pass {
        // Two-pass: seed from the shared per-query index if it is already
        // populated (reused variant — no list call); otherwise run the
        // first list page. The detail pass stays dormant until a viewport
        // is set, so opening yields `Incomplete` rows with zero detail
        // calls.
        // A refined view evaluates its query over the whole index, so a
        // shared index still being listed is one it lists to the end.
        let index_ready = state
            .index()
            .is_some_and(|i| !i.is_empty() && (i.is_complete() || !state.local_refine()));
        if !index_ready {
            // Detached: the first list page is a round trip to the source,
            // and the caller opening the view is often a UI thread. The view
            // opens `Loading`, the rows arrive with a generation bump, and
            // consumers wait for the state to leave `Loading`.
            let seed_state = state.clone();
            let list = dio.lens.runtime.spawn(async move {
                two_pass::run_list_page(seed_state.clone()).await;
                // A viewport set before the list landed found an empty
                // index and enqueued nothing, so hydration would wait for
                // the user to scroll. Re-drive it now that there are rows
                // to hydrate — the same restart the reorder path does.
                seed_state.refresh_loaded_viewport(vantage_core::Priority::Background);
            });
            state.tasks.hold(list);
        } else {
            // A populated shared index costs no query — it is already in
            // memory, so seeding from it stays inline and the view opens
            // with its rows. A refined view publishes only its refined map,
            // never the index's raw rows, and settles once its map answers
            // the query, which on an augmented value means once its sweep
            // is done; the sweep starts here.
            if state.local_refine() {
                two_pass::reseed_filtered(state).await;
                two_pass::resume_hydration(state);
            } else {
                two_pass::seed_from_index(state).await;
                state.mark_settled("open: seeded from shared index");
            }
            state.bump_generation();
        }
    } else if state.paged && !state.is_chunk_loaded() {
        // A paged view whose query the master can't answer whole. Either it
        // failed already (`replan` withheld the rows), or the reactor copies
        // the whole set and refines it; the view opens loading until then.
        if !matches!(state.serving(), Serving::Failed(_)) {
            state.reload_notify.notify_one();
        }
        state.bump_generation();
    } else if state.paged {
        seed_paged(state, dio).await?;
    } else {
        // Eager single-pass: the cache IS the row set; seed (and order)
        // from it directly.
        state.reseed_from_cache().await?;
        let seeded_rows = state.rows.read().unwrap().len();
        tracing::debug!(
            target: "vantage_diorama::cache",
            table = %dio.master.name(),
            seeded_rows,
            "cache seeded the visible map",
        );
        crate::debug::tapline!(
            state.debug_tap,
            "scenery",
            "opened {} — {} rows seeded from cache",
            if seeded_rows > 0 { "warm" } else { "cold" },
            crate::debug::num(seeded_rows),
        );
        // A seed that found rows is the answer. One that found none is
        // only an answer once no eager start is still copying
        // (`seed_complete`): a cold cache mid-copy looks exactly like an
        // empty table. Otherwise the reactor's reseed settles it, by the
        // same rule.
        if seeded_rows > 0 || dio.seed_complete.load(Ordering::SeqCst) {
            state.mark_settled("open: seeded from cache");
        }
        state.bump_generation();
    }
    Ok(())
}

/// How many rows the cache holds. A cache that can't count is opened as
/// cold, the safe reading: the view shows its loading state and the on-open
/// fetch fills it from the master. The failure is logged.
pub(super) async fn cached_count(dio: &Arc<DioInner>) -> i64 {
    match dio.cache.count().await {
        Ok(count) => count,
        Err(e) => {
            tracing::warn!(
                target: "vantage_diorama::cache",
                table = %dio.cache_table_name,
                error = %e,
                "the cache can't count its rows; the view opens as over a cold cache",
            );
            0
        }
    }
}
