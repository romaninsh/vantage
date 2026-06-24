//! Step 2 — scenery dedup registry, refcount, and structural cancellation.
//!
//! Opening the same `(conditions, sort, search)` twice hands back one shared
//! scenery (one reactor, one in-flight fetch). Releasing every handle aborts
//! the background tasks — a closing grid stops pulling — and the registry
//! entry self-heals.

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

#[then("the two table sceneries are the same object")]
async fn same_object(w: &mut DioramaWorld) {
    let a = w.scenery.as_ref().expect("first scenery not opened");
    let b = w.scenery2.as_ref().expect("second scenery not opened");
    assert!(
        Arc::ptr_eq(a, b),
        "expected the two opens to share one scenery (dedup), got distinct objects"
    );
}

#[then(regex = r"^the dio has (\d+) live table scener(?:y|ies)$")]
async fn live_count(w: &mut DioramaWorld, expected: usize) {
    let dio = w.dio.as_ref().expect("dio not created");
    let got = dio.live_table_scenery_count();
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
