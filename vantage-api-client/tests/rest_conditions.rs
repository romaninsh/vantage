//! Which conditions the REST shell takes: equality on a declared column or
//! an endpoint `{placeholder}`, with a value that renders as one URL param.

mod support;

use ciborium::Value as CborValue;
use vantage_api_client::{
    FilterStrategy, ResponseShape, RestApi, RestApiVistaFactory, RestApiVistaSpec,
};
use vantage_dataset::traits::ReadableValueSet;
use vantage_vista::{Condition, FilterOp, Vista, VistaFactory, VistaQuery};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

const SPEC: &str = r#"
name: albums
api:
  endpoint: users/{userId}/albums
columns:
  id: { type: int, flags: [id] }
  title: { type: string, flags: [title] }
"#;

fn albums() -> Vista {
    let api = RestApi::builder("https://example.com")
        .response_shape(ResponseShape::BareArray)
        .http_client(support::test_client())
        .build();
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(SPEC).unwrap();
    RestApiVistaFactory::new(api)
        .build_from_spec(spec)
        .expect("build")
}

#[test]
fn a_declared_column_is_accepted() {
    let mut vista = albums();
    vista
        .add_condition("title", FilterOp::Eq, "Quidem")
        .expect("title is declared");
    assert_eq!(vista.query().conditions.len(), 1);
}

#[test]
fn a_placeholder_column_is_accepted() {
    let mut vista = albums();
    vista
        .add_condition("userId", FilterOp::Eq, 1i64)
        .expect("userId fills the endpoint template");
    assert_eq!(vista.query().conditions.len(), 1);
}

#[test]
fn an_undeclared_column_is_an_error() {
    let mut vista = albums();
    let err = vista
        .add_condition("ghost", FilterOp::Eq, 1i64)
        .unwrap_err();
    assert!(!err.is_unsupported() && !err.is_unimplemented(), "{err}");
    assert!(
        err.to_string().contains("Unknown column for condition"),
        "{err}"
    );
    assert!(vista.query().conditions.is_empty());
}

#[test]
fn a_null_value_is_refused() {
    let mut vista = albums();
    let err = vista
        .add_condition("title", FilterOp::Eq, CborValue::Null)
        .unwrap_err();
    assert!(err.is_unsupported(), "{err}");
    assert!(vista.query().conditions.is_empty());
}

const FILTERED_SPEC: &str = r#"
name: activities
columns:
  id: { type: int, flags: [id] }
  kind: { type: string }
  mode: { type: string, flags: [hidden] }
"#;

/// Activities filtered in memory, from a server that sends every row back
/// whatever the conditions: one row has `kind`, one lacks it, and none
/// echoes the hidden `mode` request parameter.
async fn client_filtered(server: &MockServer) -> Vista {
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            { "id": 1, "kind": "ride" },
            { "id": 2 }
        ])))
        .mount(server)
        .await;
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .filter_strategy(FilterStrategy::Client)
        .http_client(support::test_client())
        .build();
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(FILTERED_SPEC).unwrap();
    RestApiVistaFactory::new(api)
        .build_from_spec(spec)
        .expect("build")
}

#[tokio::test]
async fn a_row_missing_a_visible_filtered_column_is_excluded() {
    let server = MockServer::start().await;
    let mut vista = client_filtered(&server).await;
    vista.add_condition("kind", FilterOp::Eq, "ride").unwrap();
    let ids: Vec<String> = vista.list_values().await.unwrap().into_keys().collect();
    assert_eq!(ids, ["1"]);
}

#[tokio::test]
async fn a_row_missing_a_hidden_filtered_column_is_kept() {
    let server = MockServer::start().await;
    let mut vista = client_filtered(&server).await;
    vista
        .add_condition("mode", FilterOp::Eq, "summary")
        .unwrap();
    let ids: Vec<String> = vista.list_values().await.unwrap().into_keys().collect();
    assert_eq!(ids, ["1", "2"]);
}

#[test]
fn a_null_value_is_left_for_local_evaluation() {
    let null = Condition::new("title", FilterOp::Eq, CborValue::Null);
    let query = VistaQuery::new()
        .with_condition(Condition::new("id", FilterOp::Eq, 3i64))
        .with_condition(null.clone());
    let (vista, residual) = albums().with_query(&query).expect("with_query");
    assert_eq!(residual.conditions, vec![null]);
    assert_eq!(vista.query().conditions, query.conditions[..1]);
}
