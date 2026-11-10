//! Query-parameter auth reaches every request; credentials never reach a
//! log line or an error message.

mod support;

use std::sync::{Arc, Mutex};

use ciborium::Value as CborValue;
use indexmap::IndexMap;
use reqwest::Method;
use support::test_client;
use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record as SpanRecord};
use tracing::{Event, Metadata, Subscriber};
use vantage_api_client::{ApiInsertBlock, ResponseShape, RestApi, RestApiBuilder};
use vantage_dataset::prelude::ReadableValueSet;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

/// Collects every event as one `name=value ...` line.
#[derive(Clone, Default)]
struct LogLines(Arc<Mutex<Vec<String>>>);

struct LineVisitor(String);

impl Visit for LineVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        self.0.push_str(&format!("{}={:?} ", field.name(), value));
    }
}

impl Subscriber for LogLines {
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }
    fn record(&self, _: &Id, _: &SpanRecord<'_>) {}
    fn record_follows_from(&self, _: &Id, _: &Id) {}
    fn event(&self, event: &Event<'_>) {
        let mut visitor = LineVisitor(String::new());
        event.record(&mut visitor);
        self.0.lock().unwrap().push(visitor.0);
    }
    fn enter(&self, _: &Id) {}
    fn exit(&self, _: &Id) {}
}

impl LogLines {
    fn text(&self) -> String {
        self.0.lock().unwrap().join("\n")
    }
}

fn builder(server: &MockServer) -> RestApiBuilder {
    RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .http_client(test_client())
        .debug(true)
}

fn record(name: &str) -> Record<CborValue> {
    Record::from_iter([("name".to_string(), CborValue::Text(name.to_string()))])
}

/// An error body that echoes the request back, URL, headers and body, as a
/// debugging proxy would; a success answers `[]`.
fn echo(status: u16) -> impl Fn(&Request) -> ResponseTemplate {
    move |req: &Request| {
        let body = if status < 400 {
            "[]".to_string()
        } else {
            format!(
                "{} {:?} {}",
                req.url,
                req.headers,
                String::from_utf8_lossy(&req.body)
            )
        };
        ResponseTemplate::new(status).set_body_string(body)
    }
}

async fn answer_all(server: &MockServer, status: u16) {
    for verb in ["GET", "POST"] {
        Mock::given(method(verb))
            .respond_with(echo(status))
            .mount(server)
            .await;
    }
}

fn assert_never(haystack: &str, needles: &[&str]) {
    for needle in needles {
        assert!(
            !haystack.contains(needle),
            "{needle} leaked in:\n{haystack}"
        );
    }
}

#[tokio::test]
async fn query_auth_is_added_to_every_request() {
    let server = MockServer::start().await;
    answer_all(&server, 200).await;
    let api = builder(&server).query_auth("api_secret", "s3").build();

    let table: Table<RestApi, EmptyEntity> = Table::new("things", api.clone()).with_id_column("id");
    table.list_values().await.unwrap();
    let block: ApiInsertBlock =
        serde_yaml_ng::from_str("path: collect\nquery: { mid: G-1 }").unwrap();
    api.insert_endpoint(&block, "events")
        .unwrap()
        .send(&[record("a")])
        .await
        .unwrap();

    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 2);
    let get = requests.iter().find(|r| r.method == Method::GET).unwrap();
    let post = requests.iter().find(|r| r.method == Method::POST).unwrap();
    assert!(
        get.url.query().unwrap().contains("api_secret=s3"),
        "{}",
        get.url
    );
    assert_eq!(post.url.query(), Some("mid=G-1&api_secret=s3"));
}

#[tokio::test]
async fn debug_logs_and_errors_redact_credentials() {
    let server = MockServer::start().await;
    answer_all(&server, 422).await;
    let logs = LogLines::default();
    let _guard = tracing::subscriber::set_default(logs.clone());
    let api = builder(&server)
        .query_auth("api_secret", "s3")
        .bearer_source(Arc::new(|| Some("tok-123".to_string())))
        .build();

    // A failing read.
    let table: Table<RestApi, EmptyEntity> = Table::new("things", api.clone()).with_id_column("id");
    let read_error = table.list_values().await.unwrap_err().to_string();
    assert!(read_error.contains("api_secret=***"), "{read_error}");

    // A failing request that carries an api-key header.
    let key_error = api
        .http_request(Method::POST, "/x", &[("x-api-key", "key-456")], None)
        .await
        .unwrap_err()
        .to_string();
    assert!(key_error.contains("api_secret=***"), "{key_error}");

    // A failing insert with a query value, a body secret and a status the table refuses.
    let block: ApiInsertBlock = serde_yaml_ng::from_str(
        "path: collect\nquery: { measurement_id: G-ABC123 }\naccept: [204]\nbody: '#{ n: secret.key }'",
    )
    .unwrap();
    let endpoint = api
        .insert_endpoint(&block, "events")
        .unwrap()
        .with_secrets(IndexMap::from([(
            "key".to_string(),
            "sekret-key".to_string(),
        )]));
    let insert_error = endpoint.send(&[record("a")]).await.unwrap_err().to_string();
    assert!(insert_error.contains("api_secret=***"), "{insert_error}");
    assert!(
        insert_error.contains("measurement_id=***"),
        "{insert_error}"
    );

    // A body template whose own error text would echo the secret.
    let block: ApiInsertBlock =
        serde_yaml_ng::from_str("path: collect\nbody: '#{ n: parse_int(secret.key) }'").unwrap();
    let template_error = api
        .insert_endpoint(&block, "events")
        .unwrap()
        .with_secrets(IndexMap::from([(
            "key".to_string(),
            "sekret-key".to_string(),
        )]))
        .send(&[record("a")])
        .await
        .unwrap_err()
        .to_string();
    assert!(
        template_error.contains("api.insert.body failed"),
        "{template_error}"
    );
    assert!(template_error.contains("***"), "{template_error}");

    // Debug output shows names, never values.
    let debug = format!("{endpoint:?} {api:?}");
    assert!(debug.contains("measurement_id=***"), "{debug}");
    assert!(debug.contains("key"), "{debug}");
    assert_never(&debug, &["=s3", "G-ABC123", "sekret-key", "tok-123"]);

    // A fixed Authorization value an error body echoes back.
    let fixed = builder(&server)
        .query_auth("api_secret", "s3")
        .auth("Bearer fixed-789")
        .build();
    let table: Table<RestApi, EmptyEntity> = Table::new("things", fixed).with_id_column("id");
    let fixed_error = table.list_values().await.unwrap_err().to_string();

    // A transport error: nothing is listening.
    let closed = MockServer::start().await;
    let closed_uri = closed.uri();
    drop(closed);
    let dead = RestApi::builder(closed_uri)
        .response_shape(ResponseShape::BareArray)
        .http_client(test_client())
        .query_auth("api_secret", "s3")
        .build();
    let table: Table<RestApi, EmptyEntity> = Table::new("things", dead).with_id_column("id");
    let transport_error = table.list_values().await.unwrap_err().to_string();
    assert!(
        transport_error.contains("api_secret=***"),
        "{transport_error}"
    );

    let log = logs.text();
    assert!(log.contains("api_secret=***"), "{log}");
    assert!(log.contains("Authorization: ***"), "{log}");
    assert!(log.contains("x-api-key: ***"), "{log}");
    for text in [
        &read_error,
        &key_error,
        &insert_error,
        &template_error,
        &fixed_error,
        &transport_error,
        &log,
    ] {
        assert_never(
            text,
            &[
                "=s3",
                " s3",
                "tok-123",
                "key-456",
                "fixed-789",
                "G-ABC123",
                "sekret-key",
            ],
        );
    }
}
