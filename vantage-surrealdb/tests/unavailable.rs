//! A write that never reached the server fails as `Unavailable`, the one
//! kind worth retrying; an error the server answered with does not. A write
//! whose reply was lost but which landed is still this call's `Created`.

use std::collections::VecDeque;

use async_trait::async_trait;
use ciborium::Value as CborValue;
use surreal_client::{Engine, SurrealClient, SurrealError};
use vantage_dataset::prelude::*;
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::types::AnySurrealType;
use vantage_table::table::Table;
use vantage_types::{EmptyEntity, Record};

/// An engine that fails every RPC with the error its function makes.
struct Failing(fn() -> SurrealError);

#[async_trait]
impl Engine for Failing {
    async fn send_message_cbor(
        &mut self,
        _method: &str,
        _params: CborValue,
    ) -> surreal_client::Result<CborValue> {
        Err((self.0)())
    }
}

/// An engine that answers each RPC with the next scripted reply.
struct Scripted(VecDeque<surreal_client::Result<CborValue>>);

#[async_trait]
impl Engine for Scripted {
    async fn send_message_cbor(
        &mut self,
        method: &str,
        _params: CborValue,
    ) -> surreal_client::Result<CborValue> {
        self.0
            .pop_front()
            .unwrap_or_else(|| panic!("unscripted `{method}` call"))
    }
}

fn table_over(engine: impl Engine + 'static) -> Table<SurrealDB, EmptyEntity> {
    let client = SurrealClient::new(Box::new(engine), None, None);
    Table::new("item", SurrealDB::new(client))
        .with_id_column("id")
        .with_column_of::<String>("name")
}

fn item(fail: fn() -> SurrealError) -> Table<SurrealDB, EmptyEntity> {
    table_over(Failing(fail))
}

/// One statement's successful reply holding `rows`.
fn rows(rows: Vec<CborValue>) -> surreal_client::Result<CborValue> {
    Ok(CborValue::Array(vec![CborValue::Map(vec![
        (
            CborValue::Text("status".into()),
            CborValue::Text("OK".into()),
        ),
        (CborValue::Text("result".into()), CborValue::Array(rows)),
    ])]))
}

/// The insert's reply is lost, yet the read-back finds the row: the write was
/// this call's, so it reports `Created` and after-insert hooks run.
#[tokio::test]
async fn surrealdb_landed_insert_after_a_transport_error_reports_created() {
    let stored = CborValue::Map(vec![
        (CborValue::Text("id".into()), CborValue::Text("a".into())),
        (CborValue::Text("name".into()), CborValue::Text("a".into())),
    ]);
    let table = table_over(Scripted(VecDeque::from([
        rows(vec![]),
        Err(SurrealError::Connection(
            "the connection dropped before the reply arrived".into(),
        )),
        rows(vec![stored]),
    ])));
    let record: Record<AnySurrealType> =
        [("name".to_string(), AnySurrealType::from("a".to_string()))]
            .into_iter()
            .collect();
    let inserted = table
        .insert_value(Thing::new("item", "a"), &record)
        .await
        .unwrap();
    let Inserted::Created(row) = inserted else {
        panic!("expected Created, got {inserted:?}");
    };
    assert_eq!(
        row.get("name").and_then(|v| v.try_get::<String>()),
        Some("a".to_string())
    );
}

#[tokio::test]
async fn surrealdb_transport_failures_are_unavailable() {
    let record: Record<AnySurrealType> =
        [("name".to_string(), AnySurrealType::from("a".to_string()))]
            .into_iter()
            .collect();
    let cases: [(fn() -> SurrealError, bool); 4] = [
        (
            || SurrealError::Connection("the connection is closed".into()),
            true,
        ),
        (
            || SurrealError::Timeout("no reply within 50 ms".into()),
            true,
        ),
        (
            || SurrealError::ServerError {
                code: -32000,
                message: "Database record `item:a` already exists".into(),
            },
            false,
        ),
        (|| SurrealError::Auth("invalid credentials".into()), false),
    ];
    for (fail, unavailable) in cases {
        let err = item(fail)
            .insert_value(Thing::new("item", "a"), &record)
            .await
            .unwrap_err();
        assert_eq!(err.is_unavailable(), unavailable, "{:?}: {err:?}", fail());
    }
}
