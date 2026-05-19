//! Steps for the v2 `TableScenery` features — total_provider,
//! sparse rows, viewport-driven chunk loading.

use std::sync::atomic::Ordering;

use cucumber::{given, then, when};

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
