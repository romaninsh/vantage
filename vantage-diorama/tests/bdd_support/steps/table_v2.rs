//! Steps for the v2 `TableScenery` features — total_provider,
//! sparse rows, viewport-driven chunk loading.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};
use vantage_diorama::DioEvent;

use crate::bdd_support::backend::MasterRows;
use crate::bdd_support::world::{DioramaWorld, OnLoadChunkKind, OnStartLoad, TotalProviderKind};

// ---- Givens -----------------------------------------------------------------

#[given(regex = r"^a master with (\d+) rows$")]
async fn master_with_n_rows(w: &mut DioramaWorld, n: usize) {
    let rows = MasterRows::synthetic(n);
    let master = rows.build_master_for(w).await.expect("build master vista");
    w.master = Some(master);
}

#[given(regex = r"^a lens with on_start that copies the first (\d+) rows to cache$")]
async fn on_start_first_n(w: &mut DioramaWorld, n: usize) {
    w.lens_builder.on_start_load_kind = OnStartLoad::FirstN(n);
    // Don't fire the auto-refresh on open — these scenarios pre-seed
    // the cache and assert specific event counts.
    w.lens_builder.refresh_on_open = Some(false);
}

#[given("a total_provider that returns the master count")]
async fn total_provider_master(w: &mut DioramaWorld) {
    w.lens_builder.total_provider_kind = TotalProviderKind::Master;
}

#[given("a total_provider that records calls and returns the master count")]
async fn total_provider_recorded(w: &mut DioramaWorld) {
    w.lens_builder.total_provider_kind = TotalProviderKind::MasterRecorded;
}

#[given("no total_provider is configured")]
async fn no_total_provider(w: &mut DioramaWorld) {
    w.lens_builder.total_provider_kind = TotalProviderKind::Unset;
}

#[given("an on_load_chunk callback that pulls the requested range from master")]
async fn on_load_chunk_pull(w: &mut DioramaWorld) {
    w.lens_builder.on_load_chunk_kind = OnLoadChunkKind::PullFromMaster;
}

// v4 spec wording for the same callback. Kept as a separate phrase so
// the v2 features (which use the original wording) keep working unchanged.
#[given("a lens with on_load_chunk that pulls the requested range from master")]
async fn lens_with_on_load_chunk_pull(w: &mut DioramaWorld) {
    w.lens_builder.on_load_chunk_kind = OnLoadChunkKind::PullFromMaster;
}

#[given("on_load_chunk errors on the next call")]
async fn on_load_chunk_error_once(w: &mut DioramaWorld) {
    w.spies
        .on_load_chunk_error_once
        .store(true, Ordering::SeqCst);
}

#[given("an on_load_chunk callback that records calls")]
async fn on_load_chunk_record(w: &mut DioramaWorld) {
    w.lens_builder.on_load_chunk_kind = OnLoadChunkKind::RecordCalls;
}

#[given("an on_load_chunk callback that always errors")]
async fn on_load_chunk_error(w: &mut DioramaWorld) {
    w.lens_builder.on_load_chunk_kind = OnLoadChunkKind::AlwaysError;
}

// ---- Whens ------------------------------------------------------------------

#[when(regex = r"^the table scenery viewport is set to (\d+)\.\.(\d+)$")]
async fn set_viewport(w: &mut DioramaWorld, start: usize, end: usize) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    scenery.set_viewport(start..end);
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
        scenery.set_viewport(cursor..chunk_end);
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

#[when(regex = r"^dio\.invalidate_record is called for the id at index (\d+)$")]
async fn invalidate_at_index(w: &mut DioramaWorld, idx: usize) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    let row = scenery
        .row(idx)
        .unwrap_or_else(|| panic!("no row at index {idx}"));
    // Pull the id back out of the cache by listing — easier than
    // smuggling id from the enriched record (which intentionally
    // doesn't carry the id field).
    let dio = w.dio.as_ref().expect("dio not created");
    let all = dio.cache().list_values().await.expect("cache list");
    let (id, _) = all
        .iter()
        .find(|(_, rec)| {
            // Match by all of the columns we synthesised — title is
            // unique-per-row in MasterRows::synthetic.
            row.record.get("title") == rec.get("title")
        })
        .unwrap_or_else(|| panic!("cache has no row matching index {idx}"));
    dio.invalidate_record(id.clone());
    w.settle().await;
}

// ---- Thens ------------------------------------------------------------------

#[then(regex = r"^the table scenery row_count is (\d+)$")]
async fn assert_row_count(w: &mut DioramaWorld, n: usize) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    let got = scenery.row_count();
    assert_eq!(got, n, "row_count: want {n}, got {got}");
}

#[then(regex = r"^the table scenery estimated_total is (\d+)$")]
async fn assert_estimated_total(w: &mut DioramaWorld, n: usize) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    let got = scenery.estimated_total();
    assert_eq!(got, Some(n), "estimated_total: want Some({n}), got {got:?}");
}

#[then(regex = r"^the table scenery estimated_total is Some\((\d+)\)$")]
async fn assert_estimated_total_some(w: &mut DioramaWorld, n: usize) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    let got = scenery.estimated_total();
    assert_eq!(got, Some(n), "estimated_total: want Some({n}), got {got:?}");
}

#[then(regex = r"^the table scenery has_more is (true|false)$")]
async fn assert_has_more(w: &mut DioramaWorld, expected: String) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    let got = scenery.has_more();
    let want = expected == "true";
    assert_eq!(got, want, "has_more: want {want}, got {got}");
}

#[then(regex = r"^the table scenery master capability can_fetch_page is (true|false)$")]
async fn assert_can_fetch_page(w: &mut DioramaWorld, expected: String) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    let got = scenery.master_capabilities().can_fetch_page;
    let want = expected == "true";
    assert_eq!(got, want, "master.can_fetch_page: want {want}, got {got}");
}

#[then(regex = r"^the event log contains LoadFailed \{ range: (\d+)\.\.(\d+) \}$")]
async fn assert_event_log_load_failed(w: &mut DioramaWorld, start: usize, end: usize) {
    let want = start..end;
    let events = w.snapshot_events().await;
    let found = events.iter().any(|e| match e {
        DioEvent::LoadFailed { range, .. } => *range == want,
        _ => false,
    });
    assert!(
        found,
        "LoadFailed {{ range: {want:?} }} not found in event log; got: {events:?}",
    );
}

#[then(regex = r"^the table scenery row at index (\d+) is Some$")]
async fn assert_row_some(w: &mut DioramaWorld, idx: usize) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    assert!(
        scenery.row(idx).is_some(),
        "expected row at index {idx} to be Some"
    );
}

#[then(regex = r"^the table scenery row at index (\d+) is None$")]
async fn assert_row_none(w: &mut DioramaWorld, idx: usize) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    assert!(
        scenery.row(idx).is_none(),
        "expected row at index {idx} to be None"
    );
}

#[then(regex = r"^on_load_chunk has been called (\d+) times?$")]
async fn assert_on_load_chunk_count(w: &mut DioramaWorld, n: u64) {
    let got = w.spies.on_load_chunk.load(Ordering::SeqCst);
    assert_eq!(got, n, "on_load_chunk count: want {n}, got {got}");
}

#[then(regex = r"^on_load_chunk has been called (\d+) time with range (\d+)\.\.(\d+)$")]
async fn assert_on_load_chunk_count_and_range(
    w: &mut DioramaWorld,
    n: u64,
    start: usize,
    end: usize,
) {
    let got = w.spies.on_load_chunk.load(Ordering::SeqCst);
    assert_eq!(got, n, "on_load_chunk count: want {n}, got {got}");
    let last = w.spies.last_load_chunk_range.lock().await.clone();
    assert_eq!(
        last,
        Some(start..end),
        "last on_load_chunk range: want Some({start}..{end}), got {last:?}"
    );
}

#[then(regex = r"^the last on_load_chunk range is (\d+)\.\.(\d+)$")]
async fn assert_last_range(w: &mut DioramaWorld, start: usize, end: usize) {
    let last = w.spies.last_load_chunk_range.lock().await.clone();
    assert_eq!(
        last,
        Some(start..end),
        "last on_load_chunk range: want Some({start}..{end}), got {last:?}"
    );
}

#[then(regex = r"^total_provider has been called (\d+) times?$")]
async fn assert_total_provider_count(w: &mut DioramaWorld, n: u64) {
    let got = w.spies.total_provider.load(Ordering::SeqCst);
    assert_eq!(got, n, "total_provider count: want {n}, got {got}");
}

#[then(regex = r"^the master list call count is (\d+)$")]
async fn assert_master_list_count(w: &mut DioramaWorld, n: u64) {
    let got = w.spies.master_list_calls.load(Ordering::SeqCst);
    assert_eq!(got, n, "master list calls: want {n}, got {got}");
}
