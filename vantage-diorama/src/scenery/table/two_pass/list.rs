//! The list pass: cheap rows, a page at a time, into the per-query index.

mod fetch;

use std::collections::HashMap;
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_types::Record;

use crate::dio::query_index::QueryIndex;
use crate::dio::{DioEvent, DioInner};
use crate::lens::CacheStatus;

use super::super::state::SharedRows;
use super::{cached, enriched, refine};

pub(super) use fetch::{Listed, list_page_into};

/// Seed the sparse map for `ids` starting at row index `base`, reading each
/// id's current status from the detail cache. `Complete` rows show `Fresh`,
/// everything else shows `Incomplete`. A failed view is left empty.
///
/// `read` holds rows the list page already read from the cache; a view that
/// reads the Dio's own cache takes them from there, not by a point read. A
/// sugared view reads through its wrapper.
async fn seed_rows(
    state: &Arc<SharedRows>,
    dio_inner: &Arc<DioInner>,
    base: usize,
    ids: &[String],
    mut read: HashMap<String, (Record<CborValue>, CacheStatus)>,
) {
    if state.read_cache.is_some() {
        read.clear();
    }
    for (i, id) in ids.iter().enumerate() {
        let row = match read.remove(id) {
            Some(row) => Some(row),
            None => cached(state, dio_inner, id).await,
        };
        let Some((rec, status)) = row else {
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
pub(crate) async fn seed_from_index(state: &Arc<SharedRows>) {
    let Some(dio_inner) = state.dio_weak.upgrade() else {
        return;
    };
    let Some(index) = state.index() else {
        return;
    };
    let ids = index.ids();
    seed_rows(state, &dio_inner, 0, &ids, HashMap::new()).await;
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
pub(crate) async fn run_list_page(state: Arc<SharedRows>) {
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
        Ok(Listed { base, ids, rows }) => {
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
                seed_rows(&state, &dio_inner, base, &ids, rows).await;
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
/// complete. Returns what every page added, from where the first landed.
///
/// A page that adds no id leaves an index that can't grow: a source
/// ignoring the offset repeats rows it already gave, so listing stops there
/// rather than looping.
async fn list_pages(
    state: &Arc<SharedRows>,
    dio_inner: &Arc<DioInner>,
    index: &Arc<QueryIndex>,
) -> vantage_core::Result<Listed> {
    let limit = page_limit(state, dio_inner);
    let mut listed = list_page_into(state, dio_inner, index, limit).await?;
    let mut grew = !listed.ids.is_empty();
    while grew && state.local_refine() && !index.is_complete() && !state.is_failed() {
        let page = list_page_into(state, dio_inner, index, limit).await?;
        grew = !page.ids.is_empty();
        listed.ids.extend(page.ids);
        listed.rows.extend(page.rows);
    }
    Ok(listed)
}

/// How many rows one list page asks the master for: the view's page size,
/// or, for a refined view over a master without `Window`, every row. Such a
/// master answers each page with a full listing it then cuts, and a refined
/// view lists to the end, so paging it would list the whole master once per
/// page; asking for the rest lists it once. A listing from the cache's whole
/// copy reads every row from the offset whatever this asks.
pub(super) fn page_limit(state: &SharedRows, dio_inner: &DioInner) -> usize {
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
