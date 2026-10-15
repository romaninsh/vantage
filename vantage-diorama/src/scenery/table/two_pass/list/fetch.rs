//! One list page into an index: read from the listing's pinned source, stub
//! the cache statuses of master rows, and end the listing.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use ciborium::Value as CborValue;
use vantage_types::Record;

use crate::dio::augment_passes::{ListedPage, has_augment_gap, list_page};
use crate::dio::query_index::QueryIndex;
use crate::dio::{Dio, DioInner};
use crate::lens::CacheStatus;

use super::super::super::state::SharedRows;

/// What one list page added to an index.
pub(in super::super) struct Listed {
    /// Where the novel ids landed.
    pub(in super::super) base: usize,
    /// The novel ids, in order: the index skips ids it already holds.
    pub(in super::super) ids: Vec<String>,
    /// The rows a page from the cache's whole copy read with it, by id
    /// (`ids` among them), so seeding them takes no point read. Empty for a
    /// page from the master.
    pub(in super::super) rows: HashMap<String, (Record<CborValue>, CacheStatus)>,
}

/// Fetch one list page at `index`'s tail and append its ids to `index`.
/// Shared by `run_list_page` (live index — the caller also seeds the sparse
/// map) and `refresh_index` (detached index — swapped in whole once built).
/// Touches neither `state.index` nor the sparse map.
///
/// The first page pins the listing's source: the cache's whole copy when the
/// Dio [holds one](DioInner::holds_whole_copy), read in one go from the
/// offset, else the master, page by page. Rows from the master have their
/// cache statuses stubbed; rows from the copy are the cache's own and aren't
/// written back. When the index completes, inserts deferred meanwhile join
/// its end.
///
/// An unrefined index the master listed whole in its first page stamps the
/// cache as a whole copy. One spread over several pages never does: the
/// master can change between them (a row deleted before the offset shifts
/// the next page past one), and the union of the pages then misses a row the
/// prune would drop. A larger set gets its copy from the eager start, a
/// reconcile, `copy_whole` or the augmentation refresh.
pub(in super::super) async fn list_page_into(
    state: &Arc<SharedRows>,
    dio_inner: &Arc<DioInner>,
    index: &Arc<QueryIndex>,
    limit: usize,
) -> vantage_core::Result<Listed> {
    let offset = index.len();
    let dio = Dio {
        inner: dio_inner.clone(),
    };
    let from_cache = match index.reads_cache() {
        Some(cache) => cache,
        None => {
            let mark = dio_inner.pending_flashes.mark();
            let cache = dio_inner.holds_whole_copy().await;
            index.pin_source(cache, mark)
        }
    };
    // Only allocate a request id when the tap is enabled — the one
    // correlator that ties this page's "list page dispatch" to its "list
    // page return" in the debug stream.
    let req = state.debug_tap.enabled().then(|| dio_inner.next_req());
    if !from_cache {
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
    }
    let t = std::time::Instant::now();
    let first_master_page = !from_cache && offset == 0;
    let ListedPage {
        rows,
        statuses,
        total,
    } = match list_page(&dio, offset, limit, from_cache).await {
        Ok(page) => page,
        Err(e) => {
            // A failed first page lists nothing to stamp; an open mark would
            // record every settle until the index drops. The pin stays
            // `Master(None)`, so a retry from offset 0 finds no mark and this
            // index never stamps; only a fresh index does. Deliberate: a stamp
            // needs a mark taken before its own first page.
            if first_master_page {
                index.end_listing();
            }
            return Err(e);
        }
    };
    let rows_len = rows.len();
    let mut read = HashMap::new();
    let ids = match statuses {
        Some(statuses) => rows
            .into_iter()
            .zip(statuses)
            .map(|((id, record), status)| {
                read.insert(id.clone(), (record, status));
                id
            })
            .collect(),
        None => stub_statuses(dio_inner, rows).await,
    };
    let (base, mut appended) = index.append_page(ids, limit, total);
    // Rows came back, none new, and nobody else appended meanwhile: the
    // source repeated rows from before `offset`, so it ignores the offset
    // and the index can't grow. On a windowed source, a page's worth of rows
    // inserted ahead of `offset` between two page reads looks the same and
    // completes the index early; a refresh re-lists it.
    if rows_len > 0 && appended.is_empty() && base == offset {
        index.mark_complete();
    }
    if index.is_complete() {
        appended.extend(index.flush_deferred());
    }
    // The first master page ends the mark either way: only an index that
    // page listed to its end (a short page, or the stated total; not a
    // guess) is the whole set.
    if first_master_page {
        let mark = index.end_listing();
        if let Some(mark) = mark
            && index.listed_to_end()
            && !state.local_refine()
        {
            let listed: HashSet<String> = index.ids().into_iter().collect();
            dio_inner.stamp_whole_copy(&listed, &mark).await;
        }
    }
    crate::debug::tapline!(
        state.debug_tap,
        "dio",
        "list #{} got {} ids{} in {} — {} known so far{}",
        req.unwrap_or_default(),
        crate::debug::num(rows_len),
        if from_cache {
            " from the whole copy"
        } else {
            ""
        },
        crate::debug::dur(t.elapsed().as_millis() as u64),
        crate::debug::num(index.len()),
        if index.is_complete() {
            ", that's all of them"
        } else {
            ""
        },
    );
    Ok(Listed {
        base,
        ids: appended,
        rows: read,
    })
}

/// Write each master row to the cache with its status, and return the ids
/// in page order.
///
/// The gap rule: a row whose list values already fill every augment column
/// (a file's own `size`) has nothing to fetch — stub it `Complete` so the
/// detail pass skips it.
async fn stub_statuses(
    dio_inner: &DioInner,
    rows: Vec<(String, Record<CborValue>)>,
) -> Vec<String> {
    let augmented = &dio_inner.augmented_columns;
    let gap_aware = !augmented.is_empty();
    // A row this pass writes may have hydrated between the read below and
    // the write, which then puts the list row back over the detail. The
    // sweep's memo would still call it filled and never fetch it again, so
    // each row written here forgets it; a filled row costs one cache read on
    // the next sweep.
    let forget = |id: &str| dio_inner.forget_filled(id);
    let mut ids = Vec::with_capacity(rows.len());
    for (id, rec) in rows {
        match dio_inner
            .cache
            .get_value_with_status(&id)
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
                    if augmented.contains(&k) && merged.get(&k).is_some() {
                        continue;
                    }
                    merged.insert(k, v);
                }
                let _ = dio_inner
                    .cache
                    .insert_value_with_status(&id, &merged, CacheStatus::Incomplete)
                    .await;
                forget(&id);
            }
            None => {
                let status = if gap_aware && !has_augment_gap(&rec, augmented) {
                    CacheStatus::Complete
                } else {
                    CacheStatus::Incomplete
                };
                let _ = dio_inner
                    .cache
                    .insert_value_with_status(&id, &rec, status)
                    .await;
                forget(&id);
            }
        }
        ids.push(id);
    }
    ids
}
