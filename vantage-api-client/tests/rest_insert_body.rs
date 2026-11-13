//! Insert body templates, batching, grouping and gzip encoding.

#![cfg(feature = "rhai")]

mod support;

use std::io::Read;

use ciborium::Value as CborValue;
use flate2::read::GzDecoder;
use indexmap::IndexMap;
use serde_json::{Value, json};
use support::test_client;
use vantage_api_client::{
    ApiInsertBlock, ResponseShape, RestApi, RestApiVistaFactory, RestApiVistaSpec,
    RestInsertEndpoint,
};
use vantage_dataset::traits::InsertableValueSet;
use vantage_types::Record;
use vantage_vista::VistaFactory;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

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
    // The batch carries a hostile secret: it is data, never template source.
    let server = accepting().await;
    let nasty = "a\"#{x}\\\n";
    let ep = endpoint(
        &server,
        "{ batch: 2, body: '#{ api_key: secret.key, batch: records }' }",
    )
    .await
    .unwrap()
    .with_secrets(secrets("key", nasty));

    ep.send(&[record("a", 1), record("b", 1)]).await.unwrap();
    assert_eq!(
        bodies(&server).await,
        vec![json!({
            "api_key": nasty,
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

    // A table built from YAML gets its secrets from the factory; one it
    // doesn't bind is an error naming it, not a null in the body.
    let yaml = "
name: events
api:
  insert: { body: '#{ api_key: secret.key, who: record.name, extra: record.optional_field }' }
columns:
  name: { type: string }
";
    let spec = |yaml: &str| serde_yaml_ng::from_str::<RestApiVistaSpec>(yaml).unwrap();
    let server = accepting().await;
    let factory = RestApiVistaFactory::new(
        RestApi::builder(server.uri())
            .http_client(test_client())
            .build(),
    );
    let events = factory
        .with_insert_secrets(secrets("key", "k"))
        .build_from_spec(spec(yaml))
        .unwrap();
    let row = Record::from_iter([("name".to_string(), CborValue::Text("a".to_string()))]);
    events.insert_return_id_value(&row).await.unwrap();
    assert_eq!(
        bodies(&server).await,
        vec![json!({"api_key": "k", "who": "a", "extra": null})]
    );

    let server = accepting().await;
    let unbound = RestApiVistaFactory::new(
        RestApi::builder(server.uri())
            .http_client(test_client())
            .build(),
    )
    .build_from_spec(spec(yaml))
    .unwrap();
    let err = unbound.insert_return_id_value(&row).await.unwrap_err();
    assert!(err.to_string().contains("key"), "{err}");
    assert!(bodies(&server).await.is_empty());
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
}

#[tokio::test]
async fn a_comment_or_string_naming_secret_binds_no_secret() {
    let server = accepting().await;
    let ep = endpoint(
        &server,
        r#"{ body: '#{ note: "uses secret.nope", /* secret.other */ n: record.name } // secret.third' }"#,
    )
    .await
    .unwrap();
    ep.send(&[record("a", 1)]).await.unwrap();
    assert_eq!(
        bodies(&server).await,
        vec![json!({"note": "uses secret.nope", "n": "a"})]
    );

    // Code inside a backtick interpolation is still scanned, and so is code
    // after a literal that ends the way Rhai lexes it: a backtick string
    // has no `\` escapes, a raw string none at all, and a `}` in an
    // interpolation's comment doesn't close it.
    for body in [
        r#"#{ t: `x ${secret.tok}` }"#,
        r#"#{ note: `path\`, t: secret.tok }"#,
        r##"#{ note: #"path\"#, t: secret.tok }"##,
        r###"#{ note: ##"a "# b"##, t: secret.tok }"###,
        r#"#{ t: `x ${ /* } */ secret.tok }` }"#,
    ] {
        let server = accepting().await;
        let ep = endpoint(&server, &format!("{{ body: '{body}' }}"))
            .await
            .unwrap();
        let err = ep.send(&[record("a", 1)]).await.unwrap_err().to_string();
        assert!(
            err.contains("not bound") && err.contains("tok"),
            "{body}: {err}"
        );
    }

    // A string inside an interpolation is not a reference either.
    let server = accepting().await;
    let ep = endpoint(&server, r#"{ body: '#{ t: `x ${ "secret.nope" }` }' }"#)
        .await
        .unwrap();
    ep.send(&[record("a", 1)]).await.unwrap();
    assert_eq!(bodies(&server).await, vec![json!({"t": "x secret.nope"})]);
}

#[tokio::test]
async fn a_body_secret_never_reaches_an_error_or_debug_output() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(|req: &Request| ResponseTemplate::new(422).set_body_bytes(req.body.clone()))
        .mount(&server)
        .await;
    let secret = secrets("key", "sekret-key");

    // The server echoes the body back in its error.
    let ep = endpoint(&server, "{ body: '#{ n: secret.key }' }")
        .await
        .unwrap()
        .with_secrets(secret.clone());
    let echoed = ep.send(&[record("a", 1)]).await.unwrap_err().to_string();
    assert!(echoed.contains("***"), "{echoed}");

    // The template's own error text would quote the secret.
    let ep = endpoint(&server, "{ body: '#{ n: parse_int(secret.key) }' }")
        .await
        .unwrap()
        .with_secrets(secret);
    let template = ep.send(&[record("a", 1)]).await.unwrap_err().to_string();
    assert!(template.contains("api.insert.body failed"), "{template}");
    let debug = format!("{ep:?}");
    assert!(debug.contains("key"), "{debug}");

    for text in [&echoed, &template, &debug] {
        assert!(!text.contains("sekret-key"), "secret leaked in:\n{text}");
    }
}

#[tokio::test]
async fn a_body_template_that_does_not_compile_fails_the_table_build() {
    let server = accepting().await;
    let Err(err) = endpoint(&server, "{ body: '#{ broken: ' }").await else {
        panic!("a template that does not compile must not build");
    };
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
        request
            .headers
            .get("content-encoding")
            .unwrap()
            .to_str()
            .unwrap(),
        "gzip"
    );
    let mut inflated = Vec::new();
    GzDecoder::new(&request.body[..])
        .read_to_end(&mut inflated)
        .unwrap();
    let plain_body = &plain.received_requests().await.unwrap()[0].body;
    assert_eq!(&inflated, plain_body);
}
