//! The worker pool: its size bounds the fetches in flight, and a closed
//! view's queued rows are withdrawn from it.

use std::time::Duration;

use super::{col_of, eventually, fixture};

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
    scenery.set_viewport(0..4, 0..0);

    // Both workers should dispatch a fetch and block on the closed gate.
    eventually("two fetches in flight", || {
        fx.detail.max_concurrent_gets() >= 2
    })
    .await;

    fx.gate.add_permits(64);
    eventually("all hydrated", || {
        (0..4).all(|i| col_of(&scenery, i, "size").is_some())
    })
    .await;
    assert_eq!(fx.detail.max_concurrent_gets(), 2);
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
    scenery.set_viewport(0..4, 0..0);
    eventually("worker holds r0", || fx.log().len() == 1).await;

    drop(scenery);
    fx.gate.add_permits(64);

    // The fetch already in flight (r0) completes and lands in cache. Poll
    // for it rather than sleeping a fixed interval — a slow CI runner can
    // take longer than any fixed window (`eventually` can't await the
    // async cache read, hence the inline loop).
    let mut landed = false;
    for _ in 0..200 {
        let r0 = fx.cached("r0").await.unwrap();
        if r0.get("size").is_some() {
            landed = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(landed, "in-flight fetch landed in cache");

    // Quiescence window: after the withdrawal nothing else may fetch.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let log = fx.log();
    assert_eq!(
        log,
        vec!["r0".to_string()],
        "only the in-flight fetch ran; queued ids were withdrawn"
    );
    let r1 = fx.cached("r1").await.unwrap();
    assert!(r1.get("size").is_none(), "withdrawn row was never fetched");
}
