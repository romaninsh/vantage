//! Server-side ordering for REST: with `ordering` configured the vista is
//! orderable and a pushed sort becomes the API's query param; without it
//! nothing is orderable and the sort stays client-side.

mod support;

use vantage_api_client::{
    OrderingParams, PaginationParams, ResponseShape, RestApi, RestApiVistaFactory, RestApiVistaSpec,
};
use vantage_vista::{Capability, SortDirection, VistaFactory};
use wiremock::matchers::{method, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

const SPEC: &str = r#"
name: launches
columns:
  id: { type: int, flags: [id] }
  net: { type: string }
"#;

fn factory(server: &MockServer, ordering: Option<OrderingParams>) -> RestApiVistaFactory {
    let mut api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::Wrapped {
            array_key: "results".into(),
        })
        .total_key("count")
        .pagination_params(PaginationParams::skip_limit("offset", "limit"))
        .http_client(support::test_client());
    if let Some(ordering) = ordering {
        api = api.ordering(ordering);
    }
    RestApiVistaFactory::new(api.build())
}

#[tokio::test]
async fn a_pushed_sort_becomes_the_ordering_param() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("ordering", "-net"))
        .and(query_param("offset", "0"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 1,
            "results": [{ "id": 7, "net": "2026-09-16" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(SPEC).unwrap();
    let mut vista = factory(&server, Some(OrderingParams::new("ordering", "-")))
        .build_from_spec(spec)
        .expect("build");
    assert!(vista.capabilities().has(Capability::Order));
    vista
        .set_order(Some(("net", SortDirection::Descending)))
        .expect("net is orderable");

    let page = vista.fetch_window(0, 10).await.expect("fetch");
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.total, Some(1));
}

/// A full list carries the sort too, not only a window.
#[tokio::test]
async fn a_listed_sort_becomes_the_ordering_param() {
    use vantage_dataset::traits::ReadableValueSet;

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(query_param("ordering", "-net"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "count": 1,
            "results": [{ "id": 7, "net": "2026-09-16" }]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(SPEC).unwrap();
    let vista = factory(&server, Some(OrderingParams::new("ordering", "-")))
        .build_from_spec(spec)
        .expect("build")
        .with_order("net", SortDirection::Descending)
        .expect("net is orderable");
    assert_eq!(vista.list_values().await.expect("list").len(), 1);
}

#[tokio::test]
async fn without_an_ordering_param_nothing_is_orderable() {
    let server = MockServer::start().await;
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(SPEC).unwrap();
    let mut vista = factory(&server, None).build_from_spec(spec).expect("build");
    assert!(!vista.capabilities().has(Capability::Order));
    let err = vista
        .set_order(Some(("net", SortDirection::Descending)))
        .unwrap_err();
    assert!(err.is_unsupported(), "{err}");
    assert_eq!(vista.query().order, None);
}
