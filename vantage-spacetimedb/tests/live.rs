//! Live tests against a running host. Ignored by default.
//!
//! These are what prove the HTTP layer actually talks to SpacetimeDB — the
//! fixture tests in `schema.rs` only prove the decoding. Run them with:
//!
//! ```sh
//! docker run -d --name stdb -p 3000:3000 clockworklabs/spacetime:v2.7.0-hotfix3 \
//!     start --listen-addr 0.0.0.0:3000
//! # publish a module named `smoke` (see tests/fixtures for the shape expected)
//! cargo test --test live -- --ignored --nocapture
//! ```
//!
//! Override the host and database with `SPACETIMEDB_URL` / `SPACETIMEDB_DB`.

use vantage_spacetimedb::SpacetimeDb;
use vantage_spacetimedb::schema::SchemaVersion;

fn url() -> String {
    std::env::var("SPACETIMEDB_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".to_string())
}

fn db() -> SpacetimeDb {
    let name = std::env::var("SPACETIMEDB_DB").unwrap_or_else(|_| "smoke".to_string());
    SpacetimeDb::new(url(), name)
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `smoke` module published"]
async fn reads_the_module_schema_from_a_live_host() {
    let schema = db()
        .module_schema()
        .await
        .expect("schema should be readable");

    // The negotiation should land on v10 against a current host — which is the
    // only version that can report event tables.
    assert_eq!(schema.version, SchemaVersion::V10);
    assert!(schema.event_detection());
    assert!(
        schema.tables.contains_key("person"),
        "expected the smoke module's tables, got {:?}",
        schema.tables.keys().collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `smoke` module published"]
async fn both_abi_versions_agree_on_the_table_set() {
    let db = db();
    let v10 = db.schema_at(SchemaVersion::V10).await.expect("v10");
    let v9 = db.schema_at(SchemaVersion::V9).await.expect("v9");

    let mut a: Vec<&String> = v10.tables.keys().collect();
    let mut b: Vec<&String> = v9.tables.keys().collect();
    a.sort();
    b.sort();
    assert_eq!(a, b, "the two ABIs should describe the same relations");

    // The difference between them is event detection, not content.
    assert!(v10.event_detection());
    assert!(!v9.event_detection());
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `smoke` module published"]
async fn sql_reads_public_rows_and_names_its_errors() {
    let db = db();

    let ok = db.sql("SELECT * FROM person").await.expect("valid query");
    assert!(
        ok.contains("schema") && ok.contains("rows"),
        "expected a schema+rows envelope, got: {ok}"
    );

    // A bad statement must surface the host's own parse message, not just "400".
    let err = db
        .sql("SELECT * FROM person ORDER BY score")
        .await
        .expect_err("ORDER BY is not in SpacetimeDB's SQL dialect");
    let text = err.to_string();
    assert!(
        text.len() > 40,
        "the host's explanation should be preserved, got: {text}"
    );
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `smoke` module published"]
async fn a_missing_database_says_so_usefully() {
    let db = SpacetimeDb::new(url(), "definitely-not-a-database");
    let err = db.module_schema().await.expect_err("should not resolve");
    let text = err.to_string();
    assert!(
        text.contains("published") || text.contains("404") || text.contains("Not Found"),
        "error should hint at the cause: {text}"
    );
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `cardroom` module published"]
async fn decodes_real_rows_from_the_cardroom_module() {
    use vantage_spacetimedb::value::{decode_sql_response, row_to_record};

    let db = SpacetimeDb::new(
        std::env::var("SPACETIMEDB_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into()),
        "cardroom",
    );

    // `to_act_seat` is an `Option<u64>`, which is exactly the case that cannot be
    // decoded without the schema: `[0, 1]` is `Some(1)`, not a two-element array.
    let body = db
        .sql("SELECT game_id, status, pot, to_act_seat FROM game")
        .await
        .expect("query should succeed");
    let decoded = decode_sql_response(&body).expect("rows should decode");

    assert_eq!(
        decoded.columns,
        ["game_id", "status", "pot", "to_act_seat"],
        "column names come from the schema, in declaration order"
    );
    assert!(
        !decoded.rows.is_empty(),
        "cardroom should have a game by now"
    );

    let record = row_to_record(&decoded.columns, &decoded.rows[0]);
    assert!(matches!(record["status"], ciborium::Value::Text(_)));
    // Either a seat number or null — never a raw tagged array.
    assert!(
        matches!(
            record["to_act_seat"],
            ciborium::Value::Integer(_) | ciborium::Value::Null
        ),
        "Option must unwrap, got {:?}",
        record["to_act_seat"]
    );
}

#[tokio::test]
#[ignore = "needs a running SpacetimeDB host with the `cardroom` module published"]
async fn identities_render_as_hex_not_nested_arrays() {
    use vantage_spacetimedb::value::{decode_sql_response, row_to_record};

    let db = SpacetimeDb::new(
        std::env::var("SPACETIMEDB_URL").unwrap_or_else(|_| "http://127.0.0.1:3000".into()),
        "cardroom",
    );
    let body = db
        .sql("SELECT identity, handle, bankroll FROM account")
        .await
        .expect("query should succeed");
    let decoded = decode_sql_response(&body).unwrap();
    assert!(!decoded.rows.is_empty(), "cardroom should have accounts");

    let record = row_to_record(&decoded.columns, &decoded.rows[0]);
    // `Identity` is a one-field product wrapping a u256; a grid needs the hex
    // string every other SpacetimeDB tool prints, not a nested structure.
    match &record["identity"] {
        ciborium::Value::Text(s) => assert!(s.starts_with("0x"), "expected hex, got {s}"),
        other => panic!("identity should render as hex text, got {other:?}"),
    }
}
