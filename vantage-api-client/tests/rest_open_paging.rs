//! A paged REST API with no total in its responses (Strava's
//! `athlete/activities`): configured paging params alone make the vista
//! serve windows, addressed by page; the end of the set is the short page.

mod support;

use support::test_client;
use vantage_api_client::{
    FilterStrategy, PaginationParams, ResponseShape, RestApi, RestApiVistaFactory, RestApiVistaSpec,
};
use vantage_vista::VistaFactory;
use wiremock::matchers::{method, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SPEC: &str = r#"
name: activities
columns:
  id: { type: int, flags: [id] }
  kind: { type: string }
"#;

fn rows(ids: std::ops::Range<i64>) -> serde_json::Value {
    serde_json::Value::Array(ids.map(|id| serde_json::json!({ "id": id })).collect())
}

#[tokio::test]
async fn paging_params_without_a_total_serve_windows_by_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("page", "2"))
        .and(query_param("per_page", "3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(rows(3..5)))
        .expect(1)
        .mount(&server)
        .await;

    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .pagination_params(PaginationParams::page_limit("page", "per_page"))
        .http_client(test_client())
        .build();
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(SPEC).unwrap();
    let vista = RestApiVistaFactory::new(api)
        .build_from_spec(spec)
        .expect("build");
    assert!(vista.capabilities().can_fetch_window);

    let (window, total) = vista.fetch_window_counted(3, 3).await.expect("fetch");
    assert_eq!(window.len(), 2, "a short page: the end of the set");
    assert_eq!(total, None);
}

/// The paged activities vista; `filter` overrides the default
/// [`FilterStrategy`].
fn paged_vista(server: &MockServer, filter: Option<FilterStrategy>) -> vantage_vista::Vista {
    let mut builder = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .pagination_params(PaginationParams::page_limit("page", "per_page"))
        .http_client(test_client());
    if let Some(filter) = filter {
        builder = builder.filter_strategy(filter);
    }
    let api = builder.build();
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(SPEC).unwrap();
    RestApiVistaFactory::new(api)
        .build_from_spec(spec)
        .expect("build")
}

async fn mount_page(server: &MockServer, page: &str, body: serde_json::Value) {
    Mock::given(method("GET"))
        .and(query_param("page", page))
        .and(query_param("per_page", "3"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

#[tokio::test]
async fn a_window_off_a_page_boundary_returns_the_rows_it_asks_for() {
    let server = MockServer::start().await;
    mount_page(&server, "2", rows(3..6)).await;
    mount_page(&server, "3", rows(6..9)).await;

    let (window, _) = paged_vista(&server, None)
        .fetch_window_counted(5, 3)
        .await
        .expect("fetch");
    let ids: Vec<&str> = window.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["5", "6", "7"]);
}

#[tokio::test]
async fn rows_lost_from_a_full_page_do_not_end_the_set() {
    let server = MockServer::start().await;
    // Page 1: three rows from the server, two after the client-side filter.
    mount_page(
        &server,
        "1",
        serde_json::json!([
            { "id": 1, "kind": "ride" },
            { "id": 9, "kind": "walk" },
            { "id": 2, "kind": "ride" }
        ]),
    )
    .await;
    mount_page(
        &server,
        "2",
        serde_json::json!([{ "id": 3, "kind": "ride" }, { "id": 4, "kind": "ride" }]),
    )
    .await;

    let mut vista = paged_vista(&server, Some(FilterStrategy::Client));
    vista
        .add_condition_eq("kind", ciborium::Value::Text("ride".into()))
        .unwrap();

    let (window, _) = vista.fetch_window_counted(0, 3).await.expect("fetch");
    let ids: Vec<&str> = window.iter().map(|(id, _)| id.as_str()).collect();
    assert_eq!(ids, ["1", "2", "3"], "a full window, not a short one");
}

#[test]
fn without_paging_params_or_a_total_there_are_no_windows() {
    let can_fetch_window = |api: RestApi| {
        let spec: RestApiVistaSpec = serde_yaml_ng::from_str(SPEC).unwrap();
        RestApiVistaFactory::new(api)
            .build_from_spec(spec)
            .expect("build")
            .capabilities()
            .can_fetch_window
    };
    let plain = RestApi::builder("http://example.invalid")
        .http_client(test_client())
        .build();
    assert!(!can_fetch_window(plain));
    let off = RestApi::builder("http://example.invalid")
        .pagination_params(PaginationParams::page_limit("page", "per_page"))
        .no_pagination()
        .http_client(test_client())
        .build();
    assert!(!can_fetch_window(off));
}

#[tokio::test]
async fn a_bare_array_api_answering_an_object_reads_one_row() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": 42, "name": "Afternoon Ride", "calories": 950.0
        })))
        .mount(&server)
        .await;

    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .http_client(test_client())
        .build();
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(
        "name: \"activities/42\"\ncolumns:\n  id: { type: int, flags: [id] }\n  name: { type: string }\n",
    )
    .unwrap();
    let vista = RestApiVistaFactory::new(api)
        .build_from_spec(spec)
        .expect("build");
    let rows = vantage_dataset::prelude::ReadableValueSet::list_values(&vista)
        .await
        .expect("one object is one row");
    assert_eq!(rows.len(), 1);
}
