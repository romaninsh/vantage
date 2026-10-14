//! Thens over a table scenery and the spies its loader bumps.

use std::sync::atomic::Ordering;

use cucumber::then;
use vantage_diorama::DioEvent;

use crate::bdd_support::world::DioramaWorld;

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
    // A page is a window, so the step reads the `Window` capability.
    let got = scenery
        .master_capabilities()
        .has(vantage_vista::Capability::Window);
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

/// The paged loader's `set_total` and the harness seed's remembered total
/// both state the master count; the spy counts both.
#[then(regex = r"^the master total has been stated (\d+) times?, the harness seed included$")]
async fn assert_total_provider_count(w: &mut DioramaWorld, n: u64) {
    let got = w.spies.total_provider.load(Ordering::SeqCst);
    assert_eq!(got, n, "total_provider count: want {n}, got {got}");
}

/// Counts every listing of the master: the harness seed's, an eager start's,
/// and each chunk load that pulls from the master.
#[then(regex = r"^the master has been listed (\d+) times?, the harness seed included$")]
async fn assert_master_list_count(w: &mut DioramaWorld, n: u64) {
    let got = w.spies.master_list_calls.load(Ordering::SeqCst);
    assert_eq!(got, n, "master list calls: want {n}, got {got}");
}
