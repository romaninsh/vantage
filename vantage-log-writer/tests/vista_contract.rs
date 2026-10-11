//! `vantage_vista::contract` over a JSONL log in a temporary directory.

#![cfg(feature = "vista")]

use ciborium::Value as CborValue;
use serde::{Deserialize, Serialize};
use tempfile::{TempDir, tempdir};
use vantage_log_writer::LogWriter;
use vantage_table::table::Table;
use vantage_vista::{Vista, contract};

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Event {
    name: String,
    user_id: i64,
}

/// An `events` log in a fresh directory, kept alive by the returned guard.
fn events() -> (TempDir, Vista) {
    let dir = tempdir().unwrap();
    let writer = LogWriter::new(dir.path());
    let table = Table::<LogWriter, Event>::new("events", writer.clone())
        .with_column_of::<String>("name")
        .with_column_of::<i64>("user_id");
    let vista = writer.vista_factory().from_table(table).unwrap();
    (dir, vista)
}

#[tokio::test]
async fn log_writer_contract_advertised() {
    let (_dir, vista) = events();
    contract::check_advertised(&vista).await;
    contract::check_advertised_writes(&vista).await;
}

#[tokio::test]
#[ignore = "v2 phase 1: shell not cloneable"]
async fn log_writer_contract_clone() {
    let (_dir, vista) = events();
    contract::check_clone(&vista, "name", CborValue::Text("login".into())).await;
}
