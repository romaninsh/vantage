//! Watches: listing and record streams, and the release of a closed one.

use std::time::Duration;

use vantage_diorama::ChangeEvent;

use crate::fixture::{fixture, get_stream, record};

#[tokio::test]
async fn watch_listing_streams_added_then_modified_with_augment() {
    let fx = fixture(0).await; // hold hydration until the ADDED lines are read

    let mut lines = get_stream(&fx.router, "/?offset=0&limit=2&watch=true").await;

    // Initial sweep: every visible row arrives as ADDED, un-hydrated.
    let first = lines.next().await;
    assert_eq!(first["type"], "ADDED");
    assert_eq!(first["object"]["name"], "r0");
    assert_eq!(first["object"]["size"], serde_json::Value::Null);
    let second = lines.next().await;
    assert_eq!(second["type"], "ADDED");
    assert_eq!(second["object"]["name"], "r1");

    // Release augmentation: each hydrated row streams in as MODIFIED.
    fx.gate.add_permits(64);
    let mut sizes = std::collections::BTreeMap::new();
    for _ in 0..2 {
        let event = lines.next().await;
        assert_eq!(event["type"], "MODIFIED");
        sizes.insert(
            event["object"]["name"].as_str().unwrap().to_string(),
            event["object"]["size"].as_str().unwrap().to_string(),
        );
    }
    assert_eq!(sizes["r0"], "100");
    assert_eq!(sizes["r1"], "200");
}

#[tokio::test]
async fn dropping_watch_connection_releases_the_scenery() -> vantage_core::Result<()> {
    let fx = fixture(0).await;

    let mut lines = get_stream(&fx.router, "/?offset=0&limit=2&watch=true").await;
    let _ = lines.next().await;
    assert_eq!(
        fx.dio.diagnostics().await?.sceneries.len(),
        1,
        "watch holds a scenery"
    );

    drop(lines);
    // The connection's view closes, and it was the only view on its rows.
    let mut released = false;
    for _ in 0..200 {
        if fx.dio.diagnostics().await?.sceneries.is_empty() {
            released = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(released, "scenery released after disconnect");

    // A closed view stops pulling: only the detail fetch already in flight
    // when it closed runs; the rest of its window is withdrawn.
    fx.gate.add_permits(16);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let log = fx.detail.get_log();
    assert!(log.len() <= 1, "fetches after disconnect: {log:?}");
    Ok(())
}

#[tokio::test]
async fn watch_detail_streams_record_changes() {
    let fx = fixture(1024).await;

    let mut lines = get_stream(&fx.router, "/r0?watch=true").await;
    let added = lines.next().await;
    assert_eq!(added["type"], "ADDED");
    assert_eq!(added["object"]["size"], "100", "watch opens hydrated");

    // An external change lands in the cache → the watch pushes MODIFIED.
    fx.dio
        .apply_change(ChangeEvent::Updated {
            id: "r0".into(),
            new: Some(record(&[("id", "r0"), ("modified", "t2"), ("size", "101")])),
        })
        .await
        .expect("apply_change");
    let modified = lines.next().await;
    assert_eq!(modified["type"], "MODIFIED");
    assert_eq!(modified["object"]["modified"], "t2");
    assert_eq!(modified["object"]["size"], "101");
}
