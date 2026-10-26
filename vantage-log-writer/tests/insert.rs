use std::time::Duration;

use serde::{Deserialize, Serialize};
use tempfile::tempdir;
use tokio::time::sleep;
use vantage_dataset::traits::{InsertableDataSet, ReadableValueSet, WritableValueSet};
use vantage_expressions::{Expression, ExpressiveEnum};
use vantage_log_writer::LogWriter;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
use vantage_types::{EmptyEntity, Record};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Event {
    name: String,
    user_id: i64,
    is_admin: bool,
    extra: String,
}

fn event() -> Event {
    Event {
        name: "login".into(),
        user_id: 42,
        is_admin: true,
        extra: "should be dropped".into(),
    }
}

async fn read_lines(path: &std::path::Path) -> Vec<serde_json::Value> {
    let text = tokio::fs::read_to_string(path).await.unwrap();
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn events_table(writer: &LogWriter) -> Table<LogWriter, Event> {
    Table::<LogWriter, Event>::new("events", writer.clone())
        .with_column_of::<String>("name")
        .with_column_of::<i64>("user_id")
        .with_column_of::<bool>("is_admin")
}

#[tokio::test]
async fn inserts_append_one_json_line_each_in_call_order_even_through_a_clone() {
    let dir = tempdir().unwrap();
    let table = events_table(&LogWriter::new(dir.path()));
    let clone = table.clone();

    let first = table.insert_return_id(&event()).await.unwrap();
    let second = clone.insert_return_id(&event()).await.unwrap();
    assert_eq!(first.len(), 26, "a ULID is 26 characters: {first}");
    assert_ne!(first, second);

    sleep(Duration::from_millis(100)).await;

    let rows = read_lines(&dir.path().join("events.jsonl")).await;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["id"], first);
    assert_eq!(rows[0]["name"], "login");
    assert_eq!(rows[0]["user_id"], 42);
    assert_eq!(rows[0]["is_admin"], true);
    assert!(
        rows[0].get("extra").is_none(),
        "a field outside the column list is dropped"
    );
    assert_eq!(rows[1]["id"], second);
}

#[tokio::test]
async fn insert_with_an_explicit_id_uses_it() {
    let dir = tempdir().unwrap();
    let table = Table::<LogWriter, EmptyEntity>::new("events", LogWriter::new(dir.path()))
        .with_column_of::<String>("name");

    let mut record = Record::<serde_json::Value>::new();
    record.insert("name".into(), serde_json::Value::String("hello".into()));
    let stored = WritableValueSet::insert_value(&table, "my-id", &record)
        .await
        .unwrap();
    assert_eq!(stored["id"], "my-id");

    sleep(Duration::from_millis(100)).await;

    let rows = read_lines(&dir.path().join("events.jsonl")).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], "my-id");
    assert_eq!(rows[0]["name"], "hello");
}

#[tokio::test]
async fn with_id_column_renames_the_id_field() {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path()).with_id_column("uid");
    let table = events_table(&writer);

    let id = table.insert_return_id(&event()).await.unwrap();

    sleep(Duration::from_millis(100)).await;

    let rows = read_lines(&dir.path().join("events.jsonl")).await;
    assert_eq!(rows[0]["uid"], id);
    assert!(rows[0].get("id").is_none());
}

#[tokio::test]
async fn a_declared_id_column_is_written_once_with_the_row_id() {
    let dir = tempdir().unwrap();
    let table = events_table(&LogWriter::new(dir.path())).with_column_of::<String>("id");

    let id = table.insert_return_id(&event()).await.unwrap();

    sleep(Duration::from_millis(100)).await;

    let rows = read_lines(&dir.path().join("events.jsonl")).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["id"], id);
}

fn eq_expr(column: &str, value: serde_json::Value) -> Expression<serde_json::Value> {
    Expression::new(
        "{} = {}",
        vec![
            ExpressiveEnum::Nested(Expression::new(column, vec![])),
            ExpressiveEnum::Nested(Expression::new("{}", vec![ExpressiveEnum::Scalar(value)])),
        ],
    )
}

// The writer builds no equality itself (`eq_value_condition` is the
// trait's unsupported default), so conditions are spelled by hand.
#[tokio::test]
async fn describe_condition_reads_as_the_expression() {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());
    let cond = eq_expr("parent", serde_json::json!("p1"));
    assert_eq!(writer.describe_condition(&cond), "parent = \"p1\"");
}

#[tokio::test]
async fn condition_column_names_eq_only() {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());
    let eq = eq_expr("parent", serde_json::json!("p1"));
    assert_eq!(writer.condition_column(&eq), Some("parent".to_string()));

    // The writer traverses nothing: its IN is an empty expression.
    let invoices = Table::<LogWriter, EmptyEntity>::new("invoice", writer.clone());
    let in_cond = writer.related_in_condition("invoice_id", &invoices, "id");
    assert_eq!(writer.condition_column(&in_cond), None);
}

#[tokio::test]
async fn condition_column_is_none_for_raw_expressions() {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());
    assert_eq!(
        writer.condition_column(&Expression::new("1 = 1", vec![])),
        None
    );
    let both = Expression::new(
        "{} AND {}",
        vec![
            ExpressiveEnum::Nested(eq_expr("a", serde_json::json!(1))),
            ExpressiveEnum::Nested(eq_expr("b", serde_json::json!(2))),
        ],
    );
    assert_eq!(writer.condition_column(&both), None);
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

    let res = table.get_value("any").await;
    assert!(res.is_err());
    assert_eq!(
        res.unwrap_err().kind(),
        vantage_core::ErrorKind::Unsupported
    );
}
