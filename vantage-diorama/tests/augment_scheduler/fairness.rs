//! Round-robin between views with disjoint viewports: neither view starves
//! behind the other's backlog.

use std::time::Duration;

use super::{col_of, eventually, fixture};

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
    a.set_viewport(0..4, 0..0);
    eventually("worker holds a's first row", || fx.log().len() == 1).await;

    let b = fx
        .dio
        .table_scenery()
        .columns(["id", "modified", "size"])
        .open()
        .await
        .unwrap();
    b.set_viewport(4..8, 0..0);
    tokio::time::sleep(Duration::from_millis(50)).await; // b's viewport enqueued

    fx.gate.add_permits(64);
    eventually("all eight fetched", || fx.log().len() == 8).await;

    let log = fx.log();
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

/// Two plain opens over the SAME query share their rows but stay two views:
/// each keeps its own viewport, so disjoint windows both hydrate, and their
/// fetches alternate between the two views' queues.
#[tokio::test]
async fn owned_sceneries_keep_their_own_viewports() {
    let fx = fixture(8, 1).await;

    let a = fx
        .dio
        .table_scenery()
        .columns(["id", "size"])
        .open()
        .await
        .unwrap();
    let b = fx
        .dio
        .table_scenery()
        .columns(["id", "size"])
        .open()
        .await
        .unwrap();
    #[cfg(feature = "diagnostics")]
    {
        let sceneries = fx.dio.diagnostics().await.expect("diagnostics").sceneries;
        assert_eq!(sceneries.len(), 2, "two views");
        assert!(
            sceneries.iter().all(|s| s.shared_by == 2),
            "one query, one set of rows: {sceneries:?}"
        );
    }

    a.set_viewport(0..4, 0..0);
    eventually("worker holds a's first row", || fx.log().len() == 1).await;
    b.set_viewport(4..8, 0..0);
    tokio::time::sleep(Duration::from_millis(50)).await; // b's viewport enqueued

    fx.gate.add_permits(64);
    eventually("both windows hydrate", || {
        (0..4).all(|i| col_of(&a, i, "size").is_some())
            && (4..8).all(|i| col_of(&b, i, "size").is_some())
    })
    .await;

    let log = fx.log();
    assert_eq!(log.len(), 8, "each row fetched once: {log:?}");
    let requester = |id: &str| id[1..].parse::<usize>().unwrap() / 4;
    for pair in log.windows(2) {
        assert_ne!(
            requester(&pair[0]),
            requester(&pair[1]),
            "fetches must alternate between the two views: {log:?}"
        );
    }
}
