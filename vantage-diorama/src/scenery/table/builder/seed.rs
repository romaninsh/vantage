//! Seeding freshly built rows at open, before their tasks start.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use vantage_core::Result;
use vantage_vista::Capability;

use crate::dio::DioInner;
use crate::scenery::table::plan::Serving;
use crate::scenery::table::state::SharedRows;
use crate::scenery::table::two_pass;
use crate::scenery::table::view_stats::TotalKind;

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
            // The first list page is a query against the datasource, and
            // awaiting it here put a whole round trip on the open path —
            // which is a UI thread waiting to mount. On a slow source that
            // is the entire cost of opening the page: one measured
            // SurrealDB list of 872 rows took 13.5s, and the window was
            // frozen for all of it.
            //
            // So run it detached. The view opens in `LoadState::Loading`,
            // the rows arrive with a generation bump, and consumers that
            // need rows wait for the state to leave `Loading` rather than
            // assuming `open()` implies data.
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
        // The cache IS the row set for this shape, so a seed that found
        // rows IS the answer. A seed that found none is not: a cold cache
        // with a background copy still in flight looks exactly like an
        // empty table, and calling it settled here is what made the grid
        // skip its loading state and paint "no rows" over a load that had
        // not started. Wait for the reactor's reseed in that case.
        //
        // Unless nothing more is coming — no eager start is still running
        // (`seed_complete`), so zero rows is a real answer and there will
        // be no second reseed to wait for. The reactor applies the same
        // rule.
        if seeded_rows > 0 || dio.seed_complete.load(Ordering::SeqCst) {
            state.mark_settled("open: seeded from cache");
        }
        state.bump_generation();
    }
    Ok(())
}

/// A paged view: rows arrive a window at a time, and no eager start ran for
/// this Dio.
///
/// A WARM cache is shown at once rather than thrown away: navigating back to
/// a grid whose rows are still cached must not flash a loading skeleton over
/// data we already hold. `reseed_from_cache` orders the cached rows by the
/// view's active sort, so a sorted grid opens in the right order; the
/// `refresh_on_open` force-fetch then replaces the visible window with the
/// authoritative server rows *silently* — the view is already settled, so the
/// swap carries no loading feedback. If the client sort can't reproduce the
/// server's order exactly, the first screen reshuffles once when the fetch
/// lands; the alternative is a skeleton over cached rows, which is worse. A
/// non-contiguous cache compacts here, but the forced refetch corrects the
/// first screen and scrolling corrects the rest.
///
/// A COLD cache leaves the map empty so the grid shows its loading state; the
/// on-open fetch is authoritative and fills the master's order. This is the
/// "cache is not present" case the loading skeleton exists for.
async fn seed_paged(state: &Arc<SharedRows>, dio: &Arc<DioInner>) -> Result<()> {
    // Both reads happen before the macro: the count is awaited, and a lock
    // taken inside the argument list would be held across it.
    let table = dio.master.name().to_string();
    let cached_rows = cached_count(dio).await;
    if cached_rows > 0 {
        state.reseed_from_cache().await?;
        // The remembered total is the whole table's, so a view whose query
        // narrows the set doesn't take it.
        if !state.view_narrows() {
            restore_meta_total(state, dio).await;
        }
        state.mark_settled("open: paged view seeded from warm cache");
        tracing::debug!(
            target: "vantage_diorama::cache",
            table = %table,
            cached_rows,
            "warm cache seeded the paged view; refresh-on-open replaces it silently",
        );
        crate::debug::tapline!(
            state.debug_tap,
            "scenery",
            "opened warm — {} rows already on disk",
            crate::debug::num(cached_rows.max(0) as usize),
        );
    } else {
        tracing::debug!(
            target: "vantage_diorama::cache",
            table = %table,
            cached_rows,
            "cache empty — paged view opens loading; the on-open fetch is authoritative",
        );
        crate::debug::tapline!(
            state.debug_tap,
            "scenery",
            "opened cold — nothing cached on disk yet",
        );
    }
    state.bump_generation();
    Ok(())
}

/// What the source can do decides everything that follows — which work is
/// pushed down, and which the layers above have to do themselves. Said once,
/// as the rows open, so the rest of the session reads against it.
fn announce(state: &SharedRows) {
    let caps = &state.master_capabilities;
    // Every shell counts, so `count` is never missing.
    let can: Vec<&str> = [
        Some("count"),
        caps.has(Capability::Window).then_some("window"),
        caps.has(Capability::Cursor).then_some("cursor"),
        caps.has(Capability::Order).then_some("order"),
        caps.has(Capability::Search).then_some("search"),
    ]
    .into_iter()
    .flatten()
    .collect();
    let cannot: Vec<&str> = [
        (!caps.has(Capability::Order)).then_some("order"),
        (!caps.has(Capability::Search)).then_some("search"),
    ]
    .into_iter()
    .flatten()
    .collect();
    crate::debug::tapline!(
        state.debug_tap,
        "source",
        "{}{} · {}",
        if can.is_empty() {
            "can do nothing but list".to_string()
        } else {
            format!("can {}", can.join(", "))
        },
        if cannot.is_empty() {
            String::new()
        } else {
            format!(" · cannot {}", cannot.join(", "))
        },
        if state.paged {
            format!("pages lazily, {} rows at a time", state.page_size)
        } else {
            "copied whole into the cache".to_string()
        },
    );
}

/// Restore the grand total remembered by the last session's stated fetch
/// (see `CacheTable::meta_total`), so a warm reopen advertises its final row
/// count before any fetch runs. Latched as stated — it WAS stated, last time
/// — so the unknown-total horizon rule stays out of the way; the on-open
/// refetch re-states (and re-persists) the current truth. Until then the
/// view reports it as an estimate.
async fn restore_meta_total(state: &Arc<SharedRows>, dio: &Arc<DioInner>) {
    match dio.cache.meta_total().await {
        Ok(Some(total)) => {
            state.total_ever_stated.store(true, Ordering::SeqCst);
            state.set_total_as(Some(total as usize), TotalKind::Remembered);
            tracing::debug!(
                target: "vantage_diorama::cache",
                table = %dio.master.name(),
                total,
                "restored the remembered grand total",
            );
        }
        Ok(None) => {}
        Err(e) => tracing::debug!(
            target: "vantage_diorama::cache",
            error = %e,
            "reading the remembered total failed — geometry arrives with the first fetch",
        ),
    }
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
