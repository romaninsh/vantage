use std::time::Duration;

use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use tokio::time::sleep;
use vantage_dataset::traits::{InsertableDataSet, ReadableValueSet};
use vantage_log_writer::LogWriter;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Event {
    name: String,
    user_id: i64,
    is_admin: bool,
    extra: String,
}

async fn read_lines(path: &std::path::Path) -> Vec<serde_json::Value> {
    let text = tokio::fs::read_to_string(path).await.unwrap();
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

#[tokio::test]
async fn insert_writes_jsonl_line() {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());
    let table = Table::<LogWriter, Event>::new("events", writer.clone())
        .with_column_of::<String>("name")
        .with_column_of::<i64>("user_id")
        .with_column_of::<bool>("is_admin");

    let event = Event {
        name: "login".into(),
        user_id: 42,
        is_admin: true,
        extra: "should be dropped".into(),
    };

    let id = table.insert_return_id(&event).await.unwrap();
    assert_eq!(id.len(), 26, "ULID is 26 chars: {}", id);

    sleep(Duration::from_millis(100)).await;

    let path = dir.path().join("events.jsonl");
    let rows = read_lines(&path).await;
    assert_eq!(rows.len(), 1);

    let row = &rows[0];
    assert_eq!(row["name"], "login");
    assert_eq!(row["user_id"], 42);
    assert_eq!(row["is_admin"], true);
    assert_eq!(row["id"], id);
    assert!(
        row.get("extra").is_none(),
        "fields not in column list must be dropped"
    );
}

#[tokio::test]
async fn cloned_table_shares_writer() {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());
    let t1 = Table::<LogWriter, Event>::new("events", writer.clone())
        .with_column_of::<String>("name")
        .with_column_of::<i64>("user_id")
        .with_column_of::<bool>("is_admin");
    let t2 = t1.clone();

    let e = Event {
        name: "x".into(),
        user_id: 1,
        is_admin: false,
        extra: "".into(),
    };
    let _ = t1.insert_return_id(&e).await.unwrap();
    let _ = t2.insert_return_id(&e).await.unwrap();

    sleep(Duration::from_millis(100)).await;

    let rows = read_lines(&dir.path().join("events.jsonl")).await;
    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn read_methods_return_unsupported() {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());
    let table = Table::<LogWriter, EmptyEntity>::new("events", writer);

    let res = table.list_values().await;
    assert!(res.is_err());
    assert_eq!(
        res.unwrap_err().kind(),
        vantage_core::ErrorKind::Unsupported
    );

    let res = table.get_value(&"any".to_string()).await;
    assert!(res.is_err());
    assert_eq!(
        res.unwrap_err().kind(),
        vantage_core::ErrorKind::Unsupported
    );
}

#[tokio::test]
async fn insert_with_explicit_id_uses_it() {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());
    let table = Table::<LogWriter, EmptyEntity>::new("events", writer)
        .with_column_of::<String>("name");

    use vantage_dataset::traits::WritableValueSet;
    let mut record = vantage_types::Record::<serde_json::Value>::new();
    record.insert("name".into(), serde_json::Value::String("hello".into()));
    let stored = WritableValueSet::insert_value(&table, &"my-id".to_string(), &record)
        .await
        .unwrap();
    assert_eq!(stored["id"], "my-id");

    sleep(Duration::from_millis(100)).await;

    let rows = read_lines(&dir.path().join("events.jsonl")).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "my-id");
    assert_eq!(rows[0]["name"], "hello");
}
