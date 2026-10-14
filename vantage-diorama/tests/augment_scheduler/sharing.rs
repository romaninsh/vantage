//! One flight per row: every requester of a row in flight waits on the same
//! fetch, whether it is another view or a facade read.

use std::time::Duration;

use vantage_dataset::prelude::ReadableValueSet;

use super::{col_of, eventually, fixture};

/// Two views over the same window: the row is fetched ONCE and both views
/// show the value. The second requester's ids either never queue (already in
/// flight) or settle on the worker's cache recheck — zero extra fetches.
#[tokio::test]
async fn overlapping_viewports_share_one_fetch_and_both_update() {
    let fx = fixture(2, 1).await;

    // Two views (both demanding the augment column) over one set of rows,
    // each with its own viewport.
    let a = fx
        .dio
        .table_scenery()
        .columns(["id", "size"])
        .open()
        .await
        .unwrap();
    a.set_viewport(0..2, 0..0);
    eventually("worker holds r0", || fx.log().len() == 1).await;

    let b = fx
        .dio
        .table_scenery()
        .columns(["id", "modified", "size"])
        .open()
        .await
        .unwrap();
    b.set_viewport(0..2, 0..0);
    #[cfg(feature = "diagnostics")]
    assert_eq!(
        fx.dio
            .diagnostics()
            .await
            .expect("diagnostics")
            .sceneries
            .len(),
        2,
        "two views"
    );
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
    let log = fx.log();
    assert_eq!(log.len(), 2, "each row fetched exactly once: {log:?}");
    let mut sorted = log.clone();
    sorted.sort();
    assert_eq!(sorted, vec!["r0".to_string(), "r1".to_string()]);
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
    scenery.set_viewport(0..2, 0..0);
    eventually("worker holds r0", || fx.log().len() == 1).await;

    let vista = fx.dio.cached();
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
    let log = fx.log();
    assert_eq!(
        log.iter().filter(|id| id.as_str() == "r0").count(),
        1,
        "r0 fetched exactly once across scenery and facade: {log:?}"
    );
}
