//! REST tables that insert through `api.insert`, and insert-only tables.

mod support;

use ciborium::Value as CborValue;
use serde_json::json;
use support::{mount, requests, test_client};
use vantage_api_client::{ResponseShape, RestApi, RestApiVistaFactory, RestApiVistaSpec};
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet};
use vantage_types::Record;
use vantage_vista::{Capability, Vista, VistaFactory};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn api(server: &MockServer) -> RestApi {
    RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .http_client(test_client())
        .build()
}

fn build(server: &MockServer, yaml: &str) -> vantage_core::Result<Vista> {
    let spec: RestApiVistaSpec = serde_yaml_ng::from_str(yaml).unwrap();
    RestApiVistaFactory::new(api(server)).build_from_spec(spec)
}

fn record(name: &str) -> Record<CborValue> {
    Record::from_iter([("name".to_string(), CborValue::Text(name.to_string()))])
}

const EVENTS: &str = "
name: events
api:
  insert: { method: POST }
columns:
  name: { type: string }
";

#[tokio::test]
async fn rest_insert_posts_the_record_as_json() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/events"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;

    let events = build(&server, EVENTS).unwrap();
    assert!(events.capabilities().has(Capability::Insert));
    events.insert_return_id_value(&record("x")).await.unwrap();

    let received = server.received_requests().await.unwrap();
    assert_eq!(received.len(), 1);
    let body: serde_json::Value = serde_json::from_slice(&received[0].body).unwrap();
    assert_eq!(body, json!({"name": "x"}));
}

#[tokio::test]
async fn rest_insert_accepts_only_listed_statuses() {
    let yaml = "
name: events
api:
  insert: { accept: [204] }
columns:
  name: { type: string }
";
    let server = MockServer::start().await;
    mount(&server, "POST", 204, "").await;
    let events = build(&server, yaml).unwrap();
    events.insert_return_id_value(&record("x")).await.unwrap();

    let server = MockServer::start().await;
    mount(&server, "POST", 200, "").await;
    let events = build(&server, yaml).unwrap();
    let err = events
        .insert_return_id_value(&record("x"))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("200"), "{err}");

    // A non-2xx status the table lists counts as success: a duplicate that
    // the API answers 409 is already stored.
    let server = MockServer::start().await;
    mount(&server, "POST", 409, "").await;
    let events = build(&server, &yaml.replace("[204]", "[200, 409]")).unwrap();
    events.insert_return_id_value(&record("x")).await.unwrap();
}

#[tokio::test]
async fn rest_insert_only_table_lists_nothing() {
    let yaml = "
name: events
api:
  read: false
  insert: {}
columns:
  name: { type: string }
";
    let server = MockServer::start().await;
    mount(&server, "GET", 200, "[]").await;
    let events = build(&server, yaml).unwrap();

    let caps = events.capabilities();
    assert!(caps.has(Capability::Insert));
    assert!(!caps.has(Capability::Traverse));
    assert!(!caps.has(Capability::Window));
    assert!(events.list_values().await.is_err());
    assert!(events.get_value("1").await.is_err());
    assert!(events.fetch_window(0, 10).await.is_err());
    assert!(events.count().await.is_err());
    assert_eq!(requests(&server).await, 0);
}

#[tokio::test]
async fn rest_insert_method_other_than_post_is_a_load_error() {
    let yaml = "
name: events
api:
  insert: { method: PUT }
columns:
  name: { type: string }
";
    let server = MockServer::start().await;
    let Err(err) = build(&server, yaml) else {
        panic!("PUT must not build");
    };
    assert!(
        err.to_string().contains("api.insert.method must be POST"),
        "{err}"
    );
}
