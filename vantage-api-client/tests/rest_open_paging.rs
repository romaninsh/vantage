//! A paged REST API with no total in its responses (Strava's
//! `athlete/activities`): configured paging params alone make the vista
//! serve windows, addressed by page; the end of the set is the short page.

use vantage_api_client::{
    PaginationParams, ResponseShape, RestApi, RestApiVistaFactory, RestApiVistaSpec,
};
use vantage_vista::VistaFactory;
use wiremock::matchers::{method, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SPEC: &str = r#"
name: activities
columns:
  id: { type: int, flags: [id] }
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
        .build();
    assert!(api.serves_windows());
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(SPEC).unwrap();
    let vista = RestApiVistaFactory::new(api)
        .build_from_spec(spec)
        .expect("build");
    assert!(vista.capabilities().can_fetch_window);

    let (window, total) = vista.fetch_window_counted(3, 3).await.expect("fetch");
    assert_eq!(window.len(), 2, "a short page: the end of the set");
    assert_eq!(total, None);
}

#[test]
fn without_paging_params_or_a_total_there_are_no_windows() {
    let plain = RestApi::builder("http://example.invalid").build();
    assert!(!plain.serves_windows());
    let off = RestApi::builder("http://example.invalid")
        .pagination_params(PaginationParams::page_limit("page", "per_page"))
        .no_pagination()
        .build();
    assert!(!off.serves_windows());
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
