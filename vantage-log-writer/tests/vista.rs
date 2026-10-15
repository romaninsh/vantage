#![cfg(feature = "vista")]

use std::time::Duration;

use ciborium::Value as CborValue;
use serde::{Deserialize, Serialize};
use tempfile::{TempDir, tempdir};
use tokio::time::sleep;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet};
use vantage_log_writer::LogWriter;
use vantage_table::table::Table;
use vantage_types::Record;
use vantage_vista::{Capability, FilterOp, Vista, VistaFactory, contract};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Event {
    name: String,
    user_id: i64,
}

fn events() -> (TempDir, LogWriter, Vista) {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());
    let table = Table::<LogWriter, Event>::new("events", writer.clone())
        .with_column_of::<String>("name")
        .with_column_of::<i64>("user_id");
    let vista = writer.vista_factory().from_table(table).unwrap();
    (dir, writer, vista)
}

async fn read_lines(path: &std::path::Path) -> Vec<serde_json::Value> {
    let text = tokio::fs::read_to_string(path).await.unwrap();
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
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
    assert_eq!(vista.id_column(), Some("id"));

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
async fn a_table_vista_is_insert_only_and_names_its_driver() {
    let (_dir, _writer, vista) = events();

    let caps: Vec<Capability> = vista.capabilities().iter().collect();
    assert_eq!(caps, [Capability::Insert]);
    assert_eq!(vista.driver_name(), "log-writer");
}

#[tokio::test]
async fn vista_insert_writes_declared_columns_and_drops_the_rest() {
    let (dir, _writer, vista) = events();

    let mut record = Record::<CborValue>::new();
    record.insert("name".into(), CborValue::Text("login".into()));
    record.insert("user_id".into(), CborValue::Integer(42.into()));
    record.insert("extra".into(), CborValue::Text("dropped".into()));

    let id = vista.insert_return_id_value(&record).await.unwrap();
    assert_eq!(id.len(), 26);

    sleep(Duration::from_millis(100)).await;

    let rows = read_lines(&dir.path().join("events.jsonl")).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["name"], "login");
    assert_eq!(rows[0]["user_id"], 42);
    assert_eq!(rows[0]["id"], id);
    assert!(rows[0].get("extra").is_none());
}

#[tokio::test]
async fn every_vista_read_is_unsupported_as_a_write_only_sink() {
    let (_dir, _writer, vista) = events();

    let unsupported = |res: vantage_core::Result<()>| {
        let err = res.unwrap_err();
        assert_eq!(err.kind(), vantage_core::ErrorKind::Unsupported, "{err:?}");
    };
    unsupported(vista.list_values().await.map(drop));
    unsupported(vista.get_value("any").await.map(drop));
    unsupported(vista.get_some_value().await.map(drop));
    unsupported(vista.count().await.map(drop));
    unsupported(vista.fetch_window(0, 10).await.map(drop));
    unsupported(vista.fetch_cursor(None, 10).await.map(drop));
}

#[tokio::test]
async fn preview_says_nothing_is_queried_and_why() {
    let (_dir, _writer, vista) = events();

    let preview = vista.preview();
    assert!(preview.query.is_none());
    assert!(preview.note.is_some());

    let narrowed = vista.with_condition("name", FilterOp::Eq, "login").unwrap();
    assert!(narrowed.preview().query.is_none());
}

#[tokio::test]
async fn a_narrowed_handle_records_its_condition_but_refuses_inserts() {
    let (_dir, _writer, vista) = events();

    let narrowed = vista
        .clone()
        .with_condition("name", FilterOp::Eq, "login")
        .unwrap();
    assert!(narrowed.is_narrowed());
    let err = narrowed
        .insert_return_id_value(&Record::new())
        .await
        .unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
}

#[tokio::test]
async fn a_condition_on_an_undeclared_column_is_not_unsupported() {
    let (_dir, _writer, vista) = events();

    let err = vista.with_condition("nope", FilterOp::Eq, "x").unwrap_err();
    assert!(!err.is_unsupported(), "{err:?}");
}

#[tokio::test]
async fn a_non_equality_condition_is_unsupported() {
    let (_dir, _writer, vista) = events();

    let err = vista
        .with_condition("user_id", FilterOp::Gt, 5_i64)
        .unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
}

#[tokio::test]
async fn yaml_filename_redirects_the_log_file() {
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
    vista.insert_return_id_value(&record).await.unwrap();
    sleep(Duration::from_millis(100)).await;

    assert!(dir.path().join("events_v2.jsonl").exists());
    assert!(!dir.path().join("prod_events.jsonl").exists());
}

#[tokio::test]
async fn an_unknown_key_in_the_log_writer_block_is_an_error() {
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

    let err = match writer.vista_factory().from_yaml(yaml) {
        Ok(_) => panic!("expected an error for the unknown key"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("bogus"), "{err}");
}

#[tokio::test]
async fn the_vista_satisfies_the_advertised_capability_contract() {
    let (_dir, _writer, vista) = events();

    contract::check_advertised(&vista).await;
    contract::check_advertised_writes(&vista).await;
}

#[tokio::test]
async fn a_cloned_vista_satisfies_the_clone_contract() {
    let (_dir, _writer, vista) = events();

    contract::check_clone(&vista, "name", CborValue::Text("login".into())).await;
}
