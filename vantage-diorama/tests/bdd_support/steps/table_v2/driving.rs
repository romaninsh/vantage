//! Steps that drive an open table scenery: its viewport, a random-mode
//! open, row reads, load-more, and a notify-only change.

use std::sync::atomic::Ordering;

use cucumber::{given, when};
use vantage_diorama::ChangeEvent;

use crate::bdd_support::world::DioramaWorld;

#[when(regex = r"^the table scenery viewport is set to (\d+)\.\.(\d+)$")]
async fn set_viewport(w: &mut DioramaWorld, start: usize, end: usize) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    scenery.set_viewport(start..end, 0..0);
    w.settle().await;
}

#[when(regex = r"^the table scenery is opened in random mode with page_size (\d+)$")]
async fn open_random_mode(w: &mut DioramaWorld, page_size: usize) {
    let dio = w.dio.as_ref().expect("dio not created");
    let scenery = dio
        .table_scenery()
        .page_size(page_size)
        .open()
        .await
        .expect("open table scenery (random mode)");
    w.scenery = Some(scenery);
    // refresh_on_open enqueues a viewport request that has to clear
    // the debounce timer AND complete its on_load_chunk callback
    // before subsequent steps observe a settled cache. Drive virtual
    // time and yield until both `ViewportChanged` and `RangeLoaded`
    // have landed in the event log (or until we time out — scenarios
    // that disabled refresh_on_open get an empty event log here, and
    // that's fine).
    for _ in 0..4_000 {
        if w.snapshot_events().await.len() >= 2 {
            break;
        }
        for _ in 0..40 {
            tokio::task::yield_now().await;
        }
        tokio::time::advance(std::time::Duration::from_millis(1)).await;
    }
}

/// Pre-seed the sparse map at the requested indices by firing one or
/// more viewport requests in `page_size` chunks, waiting for each to
/// settle, then rewinding the `on_load_chunk` spy so the seeding calls
/// don't count against the scenario's assertions. The loader's
/// `compute_fetch_range` is what we're actually exercising in the
/// scenario, so the Given step deliberately uses the public
/// `set_viewport` API rather than a back-door cache write.
#[given(regex = r"^the cache already contains rows (\d+)\.\.(\d+)$")]
async fn cache_already_contains(w: &mut DioramaWorld, start: usize, end: usize) {
    let baseline = w.spies.on_load_chunk.load(Ordering::SeqCst);
    let baseline_last = w.spies.last_load_chunk_range.lock().await.clone();
    let baseline_events = w.snapshot_events().await.len();
    let scenery = w.scenery.as_ref().expect("scenery not opened").clone();

    let mut cursor = start;
    while cursor < end {
        // Page size is fixed at 100 in the v4 feature Background; chunk
        // the seed accordingly so the loader actually accepts each
        // viewport (compute_fetch_range caps growth at page_size).
        let chunk_end = (cursor + 100).min(end);
        // Skip chunks already populated (e.g. by refresh_on_open).
        if (cursor..chunk_end).all(|i| scenery.row(i).is_some()) {
            cursor = chunk_end;
            continue;
        }
        let events_before = w.snapshot_events().await.len();
        scenery.set_viewport(cursor..chunk_end, 0..0);
        // Drive the paused clock until the chunk callback completes
        // (ViewportChanged + RangeLoaded both observed).
        for _ in 0..4_000 {
            if w.snapshot_events().await.len() >= events_before + 2 {
                break;
            }
            for _ in 0..40 {
                tokio::task::yield_now().await;
            }
            tokio::time::advance(std::time::Duration::from_millis(1)).await;
        }
        cursor = chunk_end;
    }

    // Rewind the counters so the scenario's Then steps see counts
    // relative to the When, not the seed.
    w.spies.on_load_chunk.store(baseline, Ordering::SeqCst);
    *w.spies.last_load_chunk_range.lock().await = baseline_last;
    w.event_log.lock().await.truncate(baseline_events);
}

#[when(regex = r"^the table scenery row at index (\d+) is queried (\d+) times?$")]
async fn query_row(w: &mut DioramaWorld, idx: usize, k: usize) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    for _ in 0..k {
        let _ = scenery.row(idx);
    }
}

#[when("request_load_more is called on the table scenery")]
async fn call_load_more(w: &mut DioramaWorld) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    scenery.request_load_more();
    w.settle().await;
}

#[when(
    regex = r"^dio\.apply_change is called with Updated for the id at index (\d+) and no record$"
)]
async fn updated_without_record_at_index(w: &mut DioramaWorld, idx: usize) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    let row = scenery
        .row(idx)
        .unwrap_or_else(|| panic!("no row at index {idx}"));
    // Pull the id back out of the cache by listing — easier than
    // smuggling id from the enriched record (which intentionally
    // doesn't carry the id field).
    let dio = w.dio.as_ref().expect("dio not created");
    let all = w
        .cache_of(dio)
        .await
        .list_values()
        .await
        .expect("cache list");
    let (id, _) = all
        .iter()
        .find(|(_, rec)| {
            // Match by all of the columns we synthesised — title is
            // unique-per-row in MasterRows::synthetic.
            row.record.get("title") == rec.get("title")
        })
        .unwrap_or_else(|| panic!("cache has no row matching index {idx}"));
    dio.apply_change(ChangeEvent::Updated {
        id: id.clone(),
        new: None,
    })
    .await
    .expect("apply_change");
    w.settle().await;
}
