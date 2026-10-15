//! Watches: listing and record streams, and the release of a closed one.

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
