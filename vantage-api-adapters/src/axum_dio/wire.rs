//! The JSON and NDJSON shapes the router puts on the wire.

use axum::body::Body;
use axum::http::header;
use axum::response::Response;
use ciborium::Value as CborValue;
use futures_util::StreamExt;
use serde_json::json;
use vantage_types::Record;

/// Project a record onto the configured columns: `{"index": …, "<name>": …}`.
pub(super) fn project(
    index: usize,
    record: &Record<CborValue>,
    columns: &[(String, String)],
) -> serde_json::Value {
    let mut object = serde_json::Map::new();
    object.insert("index".into(), json!(index));
    for (name, field) in columns {
        let value = record
            .get(field)
            .map(|v| serde_json::to_value(v).unwrap_or(serde_json::Value::Null))
            .unwrap_or(serde_json::Value::Null);
        object.insert(name.clone(), value);
    }
    serde_json::Value::Object(object)
}

/// The whole record as a JSON object — the detail endpoint's shape.
pub(super) fn record_json(record: &Record<CborValue>) -> serde_json::Value {
    serde_json::to_value(record.as_inner()).unwrap_or(serde_json::Value::Null)
}

/// One kubernetes-style watch line: `{"type":"…","object":{…}}\n`.
pub(super) fn event_line(kind: &str, object: serde_json::Value) -> String {
    format!("{}\n", json!({ "type": kind, "object": object }))
}

pub(super) fn ndjson_response(
    stream: impl futures_util::Stream<Item = String> + Send + 'static,
) -> Response {
    Response::builder()
        .header(header::CONTENT_TYPE, "application/x-ndjson")
        .body(Body::from_stream(
            stream.map(Ok::<_, std::convert::Infallible>),
        ))
        .expect("static parts are valid")
}
