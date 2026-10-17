use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use ciborium::Value as CborValue;
use serde_json::json;

use surreal_client::{DebugEngine, Engine, Result, SurrealClient};

type Sent = Arc<Mutex<Vec<(String, CborValue)>>>;

/// A request/response-only engine: answers every RPC with `reply` and keeps
/// what it was sent. It implements only the required CBOR method.
struct RecordingEngine {
    sent: Sent,
    reply: CborValue,
}

#[async_trait]
impl Engine for RecordingEngine {
    async fn send_message_cbor(&mut self, method: &str, params: CborValue) -> Result<CborValue> {
        self.sent.lock().unwrap().push((method.to_string(), params));
        Ok(self.reply.clone())
    }
}

fn recording(reply: CborValue) -> (Box<dyn Engine>, Sent) {
    let sent = Sent::default();
    let engine = RecordingEngine {
        sent: sent.clone(),
        reply,
    };
    (Box::new(engine), sent)
}

#[tokio::test]
async fn json_methods_travel_as_cbor_through_the_engine() {
    let reply = CborValue::Map(vec![(
        CborValue::Text("name".into()),
        CborValue::Text("Alice".into()),
    )]);
    let (engine, sent) = recording(reply);
    let client = SurrealClient::new(engine, None, None);

    let response = client.select("users").await.expect("select");

    assert_eq!(response, json!({"name": "Alice"}));
    assert_eq!(
        sent.lock().unwrap().as_slice(),
        &[(
            "select".to_string(),
            CborValue::Array(vec![CborValue::Text("users".into())])
        )]
    );
}

#[tokio::test]
async fn engines_without_live_support_refuse_live_queries() {
    let (engine, _) = recording(CborValue::Text(
        "11111111-2222-3333-4444-555555555555".into(),
    ));
    let client = SurrealClient::new(engine, None, None);

    assert!(client.live("users").await.is_err());
}

#[tokio::test]
async fn kill_works_on_engines_without_live_support() {
    let (engine, sent) = recording(CborValue::Null);
    let client = SurrealClient::new(engine, None, None);

    client.kill("some-id").await.expect("kill");

    let sent = sent.lock().unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, "kill");
}

#[tokio::test]
async fn debug_engine_passes_requests_and_replies_through() {
    let (inner, sent) = recording(CborValue::Array(vec![CborValue::Integer(1.into())]));
    let mut engine = DebugEngine::wrap(inner);

    let response = engine
        .send_message("query", json!(["RETURN 1"]))
        .await
        .expect("send_message");
    assert_eq!(response, json!([1]));

    let response = engine
        .send_message_cbor("version", CborValue::Array(vec![]))
        .await
        .expect("send_message_cbor");
    assert_eq!(
        response,
        CborValue::Array(vec![CborValue::Integer(1.into())])
    );

    let methods: Vec<String> = sent
        .lock()
        .unwrap()
        .iter()
        .map(|(m, _)| m.clone())
        .collect();
    assert_eq!(methods, vec!["query", "version"]);
}

#[tokio::test]
async fn debug_engine_keeps_the_inner_engines_live_support() {
    let (inner, _) = recording(CborValue::Null);
    let mut engine = DebugEngine::wrap(inner);

    assert!(engine.register_live("some-id").await.is_err());
    engine.unregister_live("some-id").await;
}
