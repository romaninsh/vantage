//! The named primitives' Rust API, run against SurrealDB.

use surreal_client::SurrealConnection;
use vantage_expressions::{ExprDataSource, Expressive};
use vantage_surrealdb::primitives::{lower, similarity};
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::surreal_return::SurrealReturn;
use vantage_surrealdb::surrealdb::SurrealDB;

async fn get_db() -> SurrealDB {
    let client = SurrealConnection::dsn("cbor://root:root@localhost:8000/bakery/types")
        .expect("Invalid DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB");
    SurrealDB::new(client)
}

/// `similarity` binds its search term as a parameter, so a term carrying
/// quotes stays one value: the query still runs and scores it.
#[tokio::test]
async fn similarity_binds_a_quoted_term_as_one_value() {
    let db = get_db().await;
    let crafted = "x\") OR true OR (\"";
    let query = SurrealReturn::new(similarity(lower(surreal_expr!("'Marty'")), crafted));

    let score = db
        .execute(&query.expr())
        .await
        .expect("the crafted term is a value, not SurrealQL");
    assert!(score.try_get::<f64>().is_some(), "{score:?}");
}
