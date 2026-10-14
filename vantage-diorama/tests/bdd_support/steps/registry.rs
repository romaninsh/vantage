//! Step 2 — the shared-rows registry and structural cancellation.
//!
//! Opening the same query twice hands back two views over one set of rows
//! (one reactor, one in-flight fetch). Releasing every handle aborts the
//! background tasks — a closing grid stops pulling — and the registry entry
//! self-heals.

use std::sync::Arc;

use cucumber::{then, when};
use vantage_diorama::DioEvent;

use crate::bdd_support::world::DioramaWorld;

#[when("the table scenery is opened again")]
async fn open_again(w: &mut DioramaWorld) {
    let dio = w.dio.as_ref().expect("dio not created");
    let scenery = dio
        .table_scenery()
        .open()
        .await
        .expect("re-open table scenery");
    w.scenery2 = Some(scenery);
    w.settle().await;
}

#[then("the two table sceneries are distinct views over the same rows")]
async fn shared_rows(w: &mut DioramaWorld) {
    let a = w.scenery.as_ref().expect("first scenery not opened");
    let b = w.scenery2.as_ref().expect("second scenery not opened");
    assert!(!Arc::ptr_eq(a, b), "every open returns a view of its own");
    let dio = w.dio.as_ref().expect("dio not created");
    let sceneries = dio.diagnostics().await.expect("diagnostics").sceneries;
    assert!(
        sceneries.len() == 2 && sceneries.iter().all(|s| s.shared_by == 2),
        "expected two views sharing one set of rows, got {sceneries:?}"
    );
}

#[then(regex = r"^the dio has (\d+) live table scener(?:y|ies)$")]
async fn live_count(w: &mut DioramaWorld, expected: usize) {
    let dio = w.dio.as_ref().expect("dio not created");
    let got = dio
        .diagnostics()
        .await
        .expect("diagnostics")
        .sceneries
        .len();
    assert_eq!(
        got, expected,
        "live table scenery count: want {expected}, got {got}"
    );
}

#[when("the table scenery handles are released")]
async fn release_handles(w: &mut DioramaWorld) {
    w.scenery = None;
    w.scenery2 = None;
    // Let the drop guard's task aborts propagate.
    w.settle().await;
}

#[then("the event log contains no RangeLoaded")]
async fn no_range_loaded(w: &mut DioramaWorld) {
    let events = w.snapshot_events().await;
    let found = events
        .iter()
        .any(|e| matches!(e, DioEvent::RangeLoaded { .. }));
    assert!(
        !found,
        "expected no RangeLoaded (load should have been cancelled), got: {events:?}"
    );
}
