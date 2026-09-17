//! Server-side ordering for REST: with `ordering` configured the vista is
//! orderable and a pushed sort becomes the API's query param; without it
//! nothing is orderable and the sort stays client-side.

use vantage_api_client::{
    OrderingParams, PaginationParams, ResponseShape, RestApi, RestApiVistaFactory, RestApiVistaSpec,
};
use vantage_vista::{SortDirection, VistaFactory};
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
        .pagination_params(PaginationParams::skip_limit("offset", "limit"));
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
    assert!(vista.capabilities().can_order);
    vista
        .add_order("net", SortDirection::Descending)
        .expect("net is orderable");

    let (rows, total) = vista.fetch_window_counted(0, 10).await.expect("fetch");
    assert_eq!(rows.len(), 1);
    assert_eq!(total, Some(1));
}

#[tokio::test]
async fn without_an_ordering_param_nothing_is_orderable() {
    let server = MockServer::start().await;
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(SPEC).unwrap();
    let mut vista = factory(&server, None).build_from_spec(spec).expect("build");
    assert!(!vista.capabilities().can_order);
    assert!(vista.add_order("net", SortDirection::Descending).is_err());
}

#[test]
fn ascending_sends_the_bare_column() {
    let params = OrderingParams::new("sort", "-");
    assert_eq!(params.value("net", SortDirection::Ascending), "net");
    assert_eq!(params.value("net", SortDirection::Descending), "-net");
}
