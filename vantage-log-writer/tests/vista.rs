#![cfg(feature = "vista")]

use std::time::Duration;

use ciborium::Value as CborValue;
use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use tokio::time::sleep;
use vantage_dataset::traits::InsertableValueSet;
use vantage_log_writer::LogWriter;
use vantage_table::table::Table;
use vantage_types::Record;
use vantage_vista::VistaFactory;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Event {
    name: String,
    user_id: i64,
}

async fn read_lines(path: &std::path::Path) -> Vec<serde_json::Value> {
    let text = tokio::fs::read_to_string(path).await.unwrap();
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[tokio::test]
async fn capabilities_advertise_insert_only() {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());
    let table = Table::<LogWriter, Event>::new("events", writer.clone())
        .with_column_of::<String>("name")
        .with_column_of::<i64>("user_id");

    let vista = writer.vista_factory().from_table(table).unwrap();
    let caps = vista.capabilities();
    assert!(caps.can_insert);
    assert!(!caps.can_count);
    assert!(!caps.can_update);
    assert!(!caps.can_delete);
    assert!(!caps.can_subscribe);
    assert_eq!(vista.driver(), "log-writer");
}

#[tokio::test]
async fn from_table_insert_via_cbor_writes_jsonl() {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());
    let table = Table::<LogWriter, Event>::new("events", writer.clone())
        .with_column_of::<String>("name")
        .with_column_of::<i64>("user_id");

    let vista = writer.vista_factory().from_table(table).unwrap();

    let mut record = Record::<CborValue>::new();
    record.insert("name".into(), CborValue::Text("login".into()));
    record.insert("user_id".into(), CborValue::Integer(42.into()));
    record.insert("extra".into(), CborValue::Text("dropped".into()));

    let id = vista.insert_return_id_value(&record).await.unwrap();
    assert_eq!(id.len(), 26);

    sleep(Duration::from_millis(100)).await;

    let rows = read_lines(&dir.path().join("events.jsonl")).await;
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row["name"], "login");
    assert_eq!(row["user_id"], 42);
    assert_eq!(row["id"], id);
    assert!(row.get("extra").is_none(), "out-of-column field must drop");
}

#[tokio::test]
async fn read_methods_return_unsupported() {
    use vantage_dataset::traits::ReadableValueSet;

    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());
    let table = Table::<LogWriter, Event>::new("events", writer.clone())
        .with_column_of::<String>("name")
        .with_column_of::<i64>("user_id");

    let vista = writer.vista_factory().from_table(table).unwrap();

    let res = vista.list_values().await;
    assert!(res.is_err());
    assert_eq!(
        res.unwrap_err().kind(),
        vantage_core::ErrorKind::Unsupported
    );

    let res = vista.get_value(&"any".to_string()).await;
    assert!(res.is_err());
    assert_eq!(
        res.unwrap_err().kind(),
        vantage_core::ErrorKind::Unsupported
    );

    let res = vista.get_count().await;
    assert!(res.is_err());
    assert_eq!(
        res.unwrap_err().kind(),
        vantage_core::ErrorKind::Unsupported
    );
}

#[tokio::test]
async fn build_from_spec_yaml_round_trip() {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());

    let yaml = r#"
name: events
columns:
  id:
    type: string
    flags: [id]
  name:
    type: string
    flags: [title]
  user_id:
    type: int
"#;

    let vista = writer.vista_factory().from_yaml(yaml).unwrap();
    assert_eq!(vista.name(), "events");
    assert_eq!(vista.get_id_column(), Some("id"));

    let mut record = Record::<CborValue>::new();
    record.insert("name".into(), CborValue::Text("hello".into()));
    record.insert("user_id".into(), CborValue::Integer(7.into()));

    let id = vista.insert_return_id_value(&record).await.unwrap();
    sleep(Duration::from_millis(100)).await;

    let rows = read_lines(&dir.path().join("events.jsonl")).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "hello");
    assert_eq!(rows[0]["user_id"], 7);
    assert_eq!(rows[0]["id"], id);
}

#[tokio::test]
async fn yaml_filename_override_redirects_file() {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());

    let yaml = r#"
name: prod_events
columns:
  id: { type: string, flags: [id] }
  name: { type: string }
log_writer:
  filename: events_v2
"#;

    let vista = writer.vista_factory().from_yaml(yaml).unwrap();
    assert_eq!(vista.name(), "prod_events");

    let mut record = Record::<CborValue>::new();
    record.insert("name".into(), CborValue::Text("x".into()));
    let _ = vista.insert_return_id_value(&record).await.unwrap();
    sleep(Duration::from_millis(100)).await;

    assert!(dir.path().join("events_v2.jsonl").exists());
    assert!(!dir.path().join("prod_events.jsonl").exists());
}

#[tokio::test]
async fn unknown_yaml_field_errors() {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());

    let yaml = r#"
name: events
columns:
  id: { type: string, flags: [id] }
log_writer:
  filename: events
  bogus: 1
"#;

    let result = writer.vista_factory().from_yaml(yaml);
    let err = match result {
        Ok(_) => panic!("expected unknown-field error"),
        Err(e) => e.to_string(),
    };
    assert!(
        err.contains("bogus") || err.contains("unknown"),
        "got: {err}"
    );
}
