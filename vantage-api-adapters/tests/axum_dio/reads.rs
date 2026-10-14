//! Plain GETs: the listing window and the hydrated detail read.

use axum::http::StatusCode;
use vantage_api_adapters::axum_dio::DioRouter;

use crate::fixture::{fixture, get_json, get_stream};

#[tokio::test]
async fn get_listing_returns_cache_window_without_fetching() {
    let fx = fixture(0).await; // gate closed — any fetch would hang, none may happen

    let (status, body) = get_json(&fx.router, "/?offset=0&limit=2").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["total"], 2);
    let items = body["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["name"], "r0");
    assert_eq!(items[0]["modified"], "t1");
    assert_eq!(items[0]["size"], serde_json::Value::Null, "not hydrated");
    assert_eq!(
        fx.detail.get_log().len(),
        0,
        "a plain GET never fetches details"
    );

    // Windowing applies.
    let (_, page2) = get_json(&fx.router, "/?offset=1&limit=1").await;
    let items = page2["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["name"], "r1");
}

#[tokio::test]
async fn get_detail_hydrates_once_and_serves_cache_after() {
    let fx = fixture(1024).await; // gate open

    let (status, body) = get_json(&fx.router, "/r0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["size"], "100", "detail GET returns the hydrated row");
    assert_eq!(body["modified"], "t1", "cheap columns survive");
    assert_eq!(fx.detail.get_log().len(), 1);

    let (_, again) = get_json(&fx.router, "/r0").await;
    assert_eq!(again["size"], "100");
    assert_eq!(
        fx.detail.get_log().len(),
        1,
        "repeat detail GET is a cache hit"
    );

    let (missing, _) = get_json(&fx.router, "/nope").await;
    assert_eq!(missing, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn detail_honours_id_map_projection_and_not_found_message() {
    let fx = fixture(1024).await;
    // Rebuild the router with the contract-shaping hooks: URLs carry a bare
    // suffix ("0"), the cache keys rows as "r0"; responses expose only a
    // renamed id; a miss serves a user-safe message.
    let router = DioRouter::new(fx.dio.clone())
        .with_id_map(|id| format!("r{id}"))
        .with_record_projection(|record| {
            serde_json::json!({
                "record_id": record.get("id").and_then(|v| v.as_text().map(str::to_string)),
            })
        })
        .with_not_found_message("Tag not found")
        .into_router();

    let (status, body) = get_json(&router, "/0").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["record_id"], "r0");
    assert!(
        body.get("modified").is_none(),
        "projection replaces the record"
    );

    let (status, body) = get_json(&router, "/9").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "Tag not found");

    // The watch stream carries the same projected object.
    let mut lines = get_stream(&router, "/0?watch=true").await;
    let added = lines.next().await;
    assert_eq!(added["type"], "ADDED");
    assert_eq!(added["object"]["record_id"], "r0");
}
