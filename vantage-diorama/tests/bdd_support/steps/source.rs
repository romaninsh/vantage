//! Step 1 — scriptable-source controls.
//!
//! The "source" here is the Mock backend's shell, retained on the World as a
//! live dataset handle (`w.source`). These steps let a scenario script a slow,
//! failing, or mutating upstream deterministically under the paused clock —
//! the in-test stand-in for the real transport the later phases build.

use std::sync::atomic::Ordering;
use std::time::Duration;

use ciborium::Value as CborValue;
use cucumber::{given, then, when};

use crate::bdd_support::world::DioramaWorld;

// ---- Givens: latency + faults ----------------------------------------------

#[given(regex = r"^the source has a read latency of (\d+) milliseconds$")]
async fn source_latency(w: &mut DioramaWorld, ms: u64) {
    w.spies.source_latency_ms.store(ms, Ordering::SeqCst);
}

#[given(regex = r"^the source fails the next (\d+) reads?$")]
async fn source_fail_reads(w: &mut DioramaWorld, n: u64) {
    w.spies.source_fail_reads.store(n, Ordering::SeqCst);
}

// ---- Whens: live dataset mutation + ms time-travel -------------------------

#[when(regex = r#"^the source record "([^"]+)" (\w+) becomes "([^"]+)"$"#)]
async fn source_mutate(w: &mut DioramaWorld, id: String, field: String, value: String) {
    let source = w
        .source
        .as_ref()
        .expect("mock source not built (Mock backend only)");
    source.set_field(&id, &field, CborValue::Text(value));
}

#[when(regex = r"^(\d+) milliseconds? pass(?:es)?$")]
async fn millis_pass(w: &mut DioramaWorld, ms: u64) {
    tokio::time::advance(Duration::from_millis(ms)).await;
    w.settle().await;
}

// ---- Then: read a row's title ----------------------------------------------

#[then(regex = r#"^the table scenery row at index (\d+) has title "([^"]+)"$"#)]
async fn row_has_title(w: &mut DioramaWorld, idx: usize, title: String) {
    let scenery = w.scenery.as_ref().expect("scenery not opened");
    let row = scenery
        .row(idx)
        .unwrap_or_else(|| panic!("no row at index {idx}"));
    let got = match row.record.get("title") {
        Some(CborValue::Text(s)) => s.clone(),
        other => panic!("row {idx} title not text: {other:?}"),
    };
    assert_eq!(got, title, "row {idx} title: want {title}, got {got}");
}
