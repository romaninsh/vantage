//! Insert body templates, batching, grouping and gzip encoding.

#![cfg(feature = "rhai")]

mod support;

use std::io::Read;

use ciborium::Value as CborValue;
use flate2::read::GzDecoder;
use indexmap::IndexMap;
use serde_json::{Value, json};
use support::test_client;
use vantage_api_client::{ApiInsertBlock, ResponseShape, RestApi, RestInsertEndpoint};
use vantage_types::Record;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn endpoint(server: &MockServer, block: &str) -> vantage_core::Result<RestInsertEndpoint> {
    let api = RestApi::builder(server.uri())
        .response_shape(ResponseShape::BareArray)
        .http_client(test_client())
        .build();
    let block: ApiInsertBlock = serde_yaml_ng::from_str(block).unwrap();
    api.insert_endpoint(&block, "events")
}

async fn accepting() -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    server
}

fn record(name: &str, client: i64) -> Record<CborValue> {
    Record::from_iter([
        ("name".to_string(), CborValue::Text(name.to_string())),
        ("client_id".to_string(), CborValue::Integer(client.into())),
    ])
}

fn secrets(key: &str, value: &str) -> IndexMap<String, String> {
    IndexMap::from([(key.to_string(), value.to_string())])
}

async fn bodies(server: &MockServer) -> Vec<Value> {
    server
        .received_requests()
        .await
        .unwrap()
        .iter()
        .map(|r| serde_json::from_slice(&r.body).unwrap())
        .collect()
}

#[tokio::test]
async fn body_template_renders_a_single_and_a_batch_body() {
    let server = accepting().await;
    let ep = endpoint(
        &server,
        "{ batch: 2, body: '#{ api_key: secret.key, batch: records }' }",
    )
    .await
    .unwrap()
    .with_secrets(secrets("key", "k"));

    ep.send(&[record("a", 1), record("b", 1)]).await.unwrap();
    assert_eq!(
        bodies(&server).await,
        vec![json!({
            "api_key": "k",
            "batch": [
                {"name": "a", "client_id": 1},
                {"name": "b", "client_id": 1}
            ]
        })]
    );

    let server = accepting().await;
    let ep = endpoint(&server, "{ body: '#{ who: record.name }' }")
        .await
        .unwrap();
    ep.send(&[record("a", 1)]).await.unwrap();
    assert_eq!(bodies(&server).await, vec![json!({"who": "a"})]);
}

#[tokio::test]
async fn a_secret_with_quotes_and_code_is_sent_as_data() {
    let server = accepting().await;
    let nasty = "a\"#{x}\\\n";
    let ep = endpoint(&server, "{ body: '#{ token: secret.token }' }")
        .await
        .unwrap()
        .with_secrets(secrets("token", nasty));
    ep.send(&[record("a", 1)]).await.unwrap();
    assert_eq!(bodies(&server).await, vec![json!({"token": nasty})]);

    let server = accepting().await;
    let ep = endpoint(
        &server,
        "{ batch: 2, body: '#{ token: secret.token, batch: records }' }",
    )
    .await
    .unwrap()
    .with_secrets(secrets("token", nasty));
    ep.send(&[record("a", 1), record("b", 1)]).await.unwrap();
    assert_eq!(
        bodies(&server).await,
        vec![json!({
            "token": nasty,
            "batch": [
                {"name": "a", "client_id": 1},
                {"name": "b", "client_id": 1}
            ]
        })]
    );
}

#[tokio::test]
async fn a_body_template_that_does_not_compile_fails_the_table_build() {
    let server = accepting().await;
    let err = endpoint(&server, "{ body: '#{ broken: ' }" )
        .await
        .err()
        .expect("a template that does not compile must not build");
    assert!(err.to_string().contains("api.insert.body"), "{err}");
}

#[tokio::test]
async fn batches_never_mix_group_by_values() {
    let server = accepting().await;
    let ep = endpoint(&server, "{ batch: 25, group_by: client_id }")
        .await
        .unwrap();
    let records: Vec<_> = (0..30).map(|i| record(&format!("r{i}"), i % 2)).collect();
    ep.send(&records).await.unwrap();

    let bodies = bodies(&server).await;
    assert_eq!(bodies.len(), 2);
    for (body, client) in bodies.iter().zip([0, 1]) {
        let rows = body.as_array().unwrap();
        assert_eq!(rows.len(), 15);
        assert!(rows.iter().all(|r| r["client_id"] == client));
    }
    assert_eq!(bodies[0][0]["name"], "r0");
    assert_eq!(bodies[0][1]["name"], "r2");
}

#[tokio::test]
async fn gzip_encoding_sets_content_encoding() {
    let block = "{ batch: 2, body: '#{ batch: records }' ";
    let plain = accepting().await;
    endpoint(&plain, &format!("{block}}}"))
        .await
        .unwrap()
        .send(&[record("a", 1), record("b", 1)])
        .await
        .unwrap();

    let zipped = accepting().await;
    endpoint(&zipped, &format!("{block}, encoding: gzip }}"))
        .await
        .unwrap()
        .send(&[record("a", 1), record("b", 1)])
        .await
        .unwrap();

    let request = &zipped.received_requests().await.unwrap()[0];
    assert_eq!(
        request.headers.get("content-encoding").unwrap().to_str().unwrap(),
        "gzip"
    );
    let mut inflated = Vec::new();
    GzDecoder::new(&request.body[..])
        .read_to_end(&mut inflated)
        .unwrap();
    let plain_body = &plain.received_requests().await.unwrap()[0].body;
    assert_eq!(&inflated, plain_body);
}
