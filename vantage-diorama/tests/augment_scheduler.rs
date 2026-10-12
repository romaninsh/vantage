//! The central augment scheduler, observed from the outside: one flight per
//! row across every open view, round-robin fairness between views with
//! disjoint viewports, a configurable worker pool, and withdrawal of queued
//! work when a view closes.
//!
//! The detail source is a gated shell: every fetch logs its id (dispatch
//! order — deterministic with the default single worker) and then blocks on a
//! semaphore the test releases permit by permit, so a test can hold the
//! worker mid-fetch while it arranges the next scene.

mod support;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ciborium::Value as CborValue;
use support::detail_shell::DetailShell;
use support::{meta, record};
use tempfile::TempDir;
use vantage_dataset::prelude::ReadableValueSet;
use vantage_diorama::{Augmentation, Detail, Dio, Fetch, Lens, MergeRule, Source, TableScenery};
use vantage_vista::mocks::MockShell;
use vantage_vista::{Catalog, Vista};

/// Master with `count` rows `r0..rN`, cheap columns only (id, modified).
fn master_vista(count: usize) -> Vista {
    let mut shell = MockShell::new();
    for i in 0..count {
        let id = format!("r{i}");
        shell = shell.with_record(&id, record(&[("id", &id), ("modified", "t1")]));
    }
    Vista::new("runs", shell.with_metadata(meta(&["id", "modified"])))
}

struct Fixture {
    dio: Dio,
    log: Arc<std::sync::Mutex<Vec<String>>>,
    gate: Arc<tokio::sync::Semaphore>,
    max_concurrent: Arc<std::sync::atomic::AtomicUsize>,
    _tmp: TempDir,
}

/// A Dio over `rows` master rows, augmented with a gated `size` detail.
async fn fixture(rows: usize, workers: usize) -> Fixture {
    let tmp = TempDir::new().unwrap();
    let mut detail_rows = indexmap::IndexMap::new();
    for i in 0..rows {
        let id = format!("r{i}");
        let size = format!("{}", (i + 1) * 100);
        detail_rows.insert(id.clone(), record(&[("id", &id), ("size", &size)]));
    }
    let detail = DetailShell::gated(detail_rows);
    let log = detail.log.clone();
    let gate = detail.gate.clone();
    let max_concurrent = detail.max_concurrent.clone();

    let lens = Arc::new(
        Lens::new()
            .cache_at(tmp.path().join("cache.redb"))
            .viewport_debounce(Duration::from_millis(1))
            .augment_workers(workers)
            .build()
            .expect("lens builds"),
    );
    let dio = lens
        .make_dio(master_vista(rows))
        .await
        .expect("make_dio")
        .augment(
            Arc::new(Catalog::new()),
            vec![Augmentation {
                detail: Detail::Fixed(Arc::new(Vista::new("sizes", detail))),
                source: Source::Id,
                fetch: Fetch::PerRow,
                merge: MergeRule {
                    columns: vec!["size".into()],
                },
            }],
        );
    Fixture {
        dio,
        log,
        gate,
        max_concurrent,
        _tmp: tmp,
    }
}

fn col_of(s: &Arc<dyn TableScenery>, i: usize, c: &str) -> Option<String> {
    s.row(i).and_then(|r| match r.record.get(c) {
        Some(CborValue::Text(t)) => Some(t.clone()),
        _ => None,
    })
}

async fn eventually(label: &str, f: impl Fn() -> bool) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("condition '{label}' not met within timeout");
}

// ---------------------------------------------------------------------------

/// Two views over the same window: the row is fetched ONCE and both views
/// show the value. The second requester's ids either never queue (already in
/// flight) or settle on the worker's cache recheck — zero extra fetches.
#[tokio::test]
async fn overlapping_viewports_share_one_fetch_and_both_update() {
    let fx = fixture(2, 1).await;

    // Distinct demands (both include the augment column) → two REAL
    // sceneries, not one deduplicated handle.
    let a = fx
        .dio
        .table_scenery()
        .columns(["id", "size"])
        .open()
        .await
        .unwrap();
    a.set_viewport(0..2);
    eventually("worker holds r0", || fx.log.lock().unwrap().len() == 1).await;

    let b = fx
        .dio
        .table_scenery()
        .columns(["id", "modified", "size"])
        .open()
        .await
        .unwrap();
    b.set_viewport(0..2);
    assert_eq!(fx.dio.live_table_scenery_count(), 2, "distinct sceneries");
    tokio::time::sleep(Duration::from_millis(50)).await; // b's viewport enqueued

    fx.gate.add_permits(16);
    eventually("both views hydrated", || {
        col_of(&a, 0, "size").is_some()
            && col_of(&a, 1, "size").is_some()
            && col_of(&b, 0, "size").is_some()
            && col_of(&b, 1, "size").is_some()
    })
    .await;

    tokio::time::sleep(Duration::from_millis(50)).await;
    let log = fx.log.lock().unwrap().clone();
    assert_eq!(log.len(), 2, "each row fetched exactly once: {log:?}");
    let mut sorted = log.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["r0".to_string(), "r1".to_string()]);
}

/// Two views over disjoint windows: with the single default worker, fetches
/// alternate between the views' queues — neither view starves behind the
/// other's backlog.
#[tokio::test]
async fn disjoint_viewports_interleave_round_robin() {
    let fx = fixture(8, 1).await;

    let a = fx
        .dio
        .table_scenery()
        .columns(["id", "size"])
        .open()
        .await
        .unwrap();
    a.set_viewport(0..4);
    eventually("worker holds a's first row", || {
        fx.log.lock().unwrap().len() == 1
    })
    .await;

    let b = fx
        .dio
        .table_scenery()
        .columns(["id", "modified", "size"])
        .open()
        .await
        .unwrap();
    b.set_viewport(4..8);
    tokio::time::sleep(Duration::from_millis(50)).await; // b's viewport enqueued

    fx.gate.add_permits(64);
    eventually("all eight fetched", || fx.log.lock().unwrap().len() == 8).await;

    let log = fx.log.lock().unwrap().clone();
    let requester = |id: &str| {
        let n: usize = id[1..].parse().unwrap();
        n / 4 // r0..r3 → view A, r4..r7 → view B
    };
    for pair in log.windows(2) {
        assert_ne!(
            requester(&pair[0]),
            requester(&pair[1]),
            "fetches must alternate between the two views: {log:?}"
        );
    }
}

/// `augment_workers(2)` runs two detail fetches at the same time.
#[tokio::test]
async fn two_workers_fetch_in_parallel() {
    let fx = fixture(4, 2).await;

    let scenery = fx
        .dio
        .table_scenery()
        .columns(["id", "size"])
        .open()
        .await
        .unwrap();
    scenery.set_viewport(0..4);

    // Both workers should dispatch a fetch and block on the closed gate.
    eventually("two fetches in flight", || {
        fx.max_concurrent.load(Ordering::SeqCst) >= 2
    })
    .await;

    fx.gate.add_permits(64);
    eventually("all hydrated", || {
        (0..4).all(|i| col_of(&scenery, i, "size").is_some())
    })
    .await;
    assert_eq!(fx.max_concurrent.load(Ordering::SeqCst), 2);
}

/// Closing a view withdraws its queued rows: only the fetch already in
/// flight completes (paid-for work is kept); the rest stay unhydrated.
#[tokio::test]
async fn dropping_a_scenery_withdraws_its_queued_ids() {
    let fx = fixture(4, 1).await;

    let scenery = fx
        .dio
        .table_scenery()
        .columns(["id", "size"])
        .open()
        .await
        .unwrap();
    scenery.set_viewport(0..4);
    eventually("worker holds r0", || fx.log.lock().unwrap().len() == 1).await;

    drop(scenery);
    fx.gate.add_permits(64);

    // The fetch already in flight (r0) completes and lands in cache. Poll
    // for it rather than sleeping a fixed interval — a slow CI runner can
    // take longer than any fixed window (`eventually` can't await the
    // async cache read, hence the inline loop).
    let mut landed = false;
    for _ in 0..200 {
        let r0 = fx.dio.cache().get_value("r0").await.unwrap().unwrap();
        if r0.get("size").is_some() {
            landed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(landed, "in-flight fetch landed in cache");

    // Quiescence window: after the withdrawal nothing else may fetch.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let log = fx.log.lock().unwrap().clone();
    assert_eq!(
        log,
        vec!["r0".to_string()],
        "only the in-flight fetch ran; queued ids were withdrawn"
    );
    let r1 = fx.dio.cache().get_value("r1").await.unwrap().unwrap();
    assert!(r1.get("size").is_none(), "withdrawn row was never fetched");
}

/// Two `.exclusive()` views over the SAME query and demand stay separate
/// sceneries: each keeps its own viewport, so disjoint windows both hydrate
/// (a shared scenery would only serve the last-set window).
#[tokio::test]
async fn exclusive_sceneries_keep_their_own_viewports() {
    let fx = fixture(8, 1).await;
    fx.gate.add_permits(64);

    let a = fx
        .dio
        .table_scenery()
        .columns(["id", "size"])
        .exclusive()
        .open()
        .await
        .unwrap();
    let b = fx
        .dio
        .table_scenery()
        .columns(["id", "size"])
        .exclusive()
        .open()
        .await
        .unwrap();
    assert_eq!(
        fx.dio.live_table_scenery_count(),
        2,
        "identical queries, but exclusive views never share"
    );

    a.set_viewport(0..2);
    b.set_viewport(4..6);
    eventually("both windows hydrate", || {
        col_of(&a, 0, "size").is_some()
            && col_of(&a, 1, "size").is_some()
            && col_of(&b, 4, "size").is_some()
            && col_of(&b, 5, "size").is_some()
    })
    .await;
}

/// A facade read blocking on a row another view is already fetching does not
/// fetch it again — it waits on the same flight and returns the hydrated row.
#[tokio::test]
async fn facade_hydrate_shares_in_flight_fetch_with_scenery() {
    let fx = fixture(2, 1).await;

    let scenery = fx
        .dio
        .table_scenery()
        .columns(["id", "size"])
        .open()
        .await
        .unwrap();
    scenery.set_viewport(0..2);
    eventually("worker holds r0", || fx.log.lock().unwrap().len() == 1).await;

    let vista = fx.dio.vista();
    let read = tokio::spawn(async move { vista.get_value("r0").await });
    tokio::time::sleep(Duration::from_millis(50)).await; // read joins the r0 flight

    fx.gate.add_permits(16);
    let row = read
        .await
        .expect("facade task")
        .expect("facade read")
        .expect("row exists");
    assert_eq!(
        row.get("size").and_then(|v| v.as_text()).map(String::from),
        Some("100".into()),
        "facade read returns the hydrated row"
    );

    eventually("scenery row settles too", || {
        col_of(&scenery, 0, "size").is_some()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let log = fx.log.lock().unwrap().clone();
    assert_eq!(
        log.iter().filter(|id| id.as_str() == "r0").count(),
        1,
        "r0 fetched exactly once across scenery and facade: {log:?}"
    );
}
