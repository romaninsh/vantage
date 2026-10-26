//! Which conditions the REST shell takes: equality on a declared column or
//! an endpoint `{placeholder}`, with a value that renders as one URL param.

mod support;

use ciborium::Value as CborValue;
use vantage_api_client::{
    FilterStrategy, ResponseShape, RestApi, RestApiVistaFactory, RestApiVistaSpec,
};
use vantage_dataset::traits::ReadableValueSet;
use vantage_expressions::{Expression, ExpressiveEnum};
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::EmptyEntity;
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
fn describe_condition_reads_as_the_expression() {
    let api = RestApi::new("https://example.com");
    let cond = api
        .eq_value_condition("parent", CborValue::Text("p1".into()))
        .unwrap();
    assert_eq!(api.describe_condition(&cond), "parent = \"p1\"");
}

#[test]
fn condition_column_names_eq_and_in() {
    let api = RestApi::new("https://example.com");
    let eq = api
        .eq_value_condition("parent", CborValue::Text("p1".into()))
        .unwrap();
    assert_eq!(api.condition_column(&eq), Some("parent".to_string()));

    let invoices = Table::<RestApi, EmptyEntity>::new("invoice", api.clone()).with_id_column("id");
    let in_cond = api.related_in_condition("invoice_id", &invoices, "id");
    assert_eq!(
        api.condition_column(&in_cond),
        Some("invoice_id".to_string())
    );
}

/// A URL filter param holds one value, so only a one-value list narrows (as
/// the same `field = value` an equality sends); any other list is refused
/// rather than dropped from the request.
#[tokio::test]
async fn in_values_condition_narrows_to_one_value_and_refuses_lists() {
    let server = MockServer::start().await;
    support::mount(&server, "GET", 200, r#"[{"id":1}]"#).await;
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .http_client(support::test_client())
        .build();
    let one = api
        .in_values_condition("userId", vec![CborValue::Integer(7.into())])
        .unwrap();
    Table::<RestApi, EmptyEntity>::new("albums", api.clone())
        .with_id_column("id")
        .with_condition(one)
        .list_values()
        .await
        .unwrap();
    let received = server.received_requests().await.unwrap();
    assert_eq!(received[0].url.query(), Some("userId=7"));

    for list in [
        vec![],
        vec![CborValue::Integer(1.into()), CborValue::Integer(2.into())],
    ] {
        let refused = api.in_values_condition("userId", list);
        assert!(refused.is_err_and(|e| e.is_unsupported()));
    }
}

#[test]
fn condition_column_names_an_in_values_condition() {
    let api = RestApi::new("https://example.com");
    let cond = api
        .in_values_condition("invoice_id", vec![CborValue::Text("i1".into())])
        .unwrap();
    assert_eq!(api.condition_column(&cond), Some("invoice_id".to_string()));
}

#[test]
fn condition_column_is_none_for_raw_expressions() {
    let api = RestApi::new("https://example.com");
    assert_eq!(
        api.condition_column(&Expression::new("1 = 1", vec![])),
        None
    );
    let a = api
        .eq_value_condition("a", CborValue::Text("x".into()))
        .unwrap();
    let b = api
        .eq_value_condition("b", CborValue::Text("y".into()))
        .unwrap();
    let both = Expression::new(
        "{} AND {}",
        vec![ExpressiveEnum::Nested(a), ExpressiveEnum::Nested(b)],
    );
    assert_eq!(api.condition_column(&both), None);
}

/// The REST shell takes equality on a declared column or an endpoint
/// `{placeholder}`. An undeclared column is a plain error; a dotted path
/// is unimplemented and a null value unsupported. None of the three is
/// added.
#[test]
fn add_condition_takes_declared_and_placeholder_columns() {
    let mut vista = albums();
    vista
        .add_condition("title", FilterOp::Eq, "Quidem")
        .expect("title is declared");
    vista
        .add_condition("userId", FilterOp::Eq, 1i64)
        .expect("userId fills the endpoint template");

    let undeclared = vista
        .add_condition("ghost", FilterOp::Eq, 1i64)
        .unwrap_err();
    assert!(
        !undeclared.is_unsupported() && !undeclared.is_unimplemented(),
        "{undeclared}"
    );
    assert!(
        undeclared
            .to_string()
            .contains("Unknown column for condition"),
        "{undeclared}"
    );
    let dotted = vista
        .add_condition("title.first", FilterOp::Eq, "Quidem")
        .unwrap_err();
    assert!(dotted.is_unimplemented(), "{dotted}");
    let null = vista
        .add_condition("title", FilterOp::Eq, CborValue::Null)
        .unwrap_err();
    assert!(null.is_unsupported(), "{null}");

    assert_eq!(vista.query().conditions.len(), 2);
}

/// What the shell can't send, a dotted path or a null value, `with_query`
/// hands back for the caller to evaluate over a complete set.
#[test]
fn with_query_leaves_dotted_paths_and_nulls_for_local_evaluation() {
    let dotted = Condition::new("title.first", FilterOp::Eq, "Quidem");
    let null = Condition::new("title", FilterOp::Eq, CborValue::Null);
    let query = VistaQuery::new()
        .with_condition(Condition::new("id", FilterOp::Eq, 3i64))
        .with_condition(dotted.clone())
        .with_condition(null.clone());
    let (vista, residual) = albums().with_query(&query).expect("with_query");
    assert_eq!(residual.conditions, vec![dotted, null]);
    assert_eq!(vista.query().conditions, query.conditions[..1]);
}

const FILTERED_SPEC: &str = r#"
name: activities
columns:
  id: { type: int, flags: [id] }
  kind: { type: string }
  note: { type: string, flags: [hidden] }
  mode: { type: string, flags: [request_param] }
"#;

/// Activities filtered in memory, from a server that sends every row back
/// whatever the conditions: one row has `kind` and the hidden `note`, one
/// lacks both, and none echoes the `mode` request parameter.
async fn client_filtered(server: &MockServer) -> Vista {
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            { "id": 1, "kind": "ride", "note": "x" },
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

/// Filtered in memory, a row lacking a filtered column is excluded, a hidden
/// one included (`Hidden` is display only: a hidden column is still a row
/// field), but not a `request_param` column, which the server consumes and
/// never echoes back.
#[tokio::test]
async fn a_row_missing_a_filtered_column_is_excluded_unless_it_is_a_request_param() {
    let server = MockServer::start().await;
    let mut by_kind = client_filtered(&server).await;
    let mut by_note = client_filtered(&server).await;
    let mut by_mode = client_filtered(&server).await;
    by_kind.add_condition("kind", FilterOp::Eq, "ride").unwrap();
    by_note.add_condition("note", FilterOp::Eq, "x").unwrap();
    by_mode
        .add_condition("mode", FilterOp::Eq, "summary")
        .unwrap();

    let ids: Vec<String> = by_kind.list_values().await.unwrap().into_keys().collect();
    assert_eq!(ids, ["1"]);
    let ids: Vec<String> = by_note.list_values().await.unwrap().into_keys().collect();
    assert_eq!(ids, ["1"]);
    let ids: Vec<String> = by_mode.list_values().await.unwrap().into_keys().collect();
    assert_eq!(ids, ["1", "2"]);
}
