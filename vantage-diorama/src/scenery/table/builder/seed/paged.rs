//! Seeding a paged view at open, and saying what its source can do.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use vantage_core::Result;
use vantage_vista::Capability;

use crate::dio::DioInner;
use crate::scenery::table::state::SharedRows;
use crate::scenery::table::view_stats::TotalKind;

use super::cached_count;

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
pub(super) async fn seed_paged(state: &Arc<SharedRows>, dio: &Arc<DioInner>) -> Result<()> {
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
pub(super) fn announce(state: &SharedRows) {
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
