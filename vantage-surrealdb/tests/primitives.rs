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

/// The value `code`'s expression reads, as the database returns it.
#[cfg(feature = "rhai")]
async fn read_rhai(code: &str) -> vantage_surrealdb::AnySurrealType {
    let host = vantage_rhai::Host::builder(vantage_rhai::Limits::background())
        .vocab_fn(vantage_surrealdb::rhai_engine::register_surreal_onto)
        .build();
    let e = host
        .compile(&vantage_rhai::Block::from(code))
        .and_then(|script| {
            script.eval_as::<vantage_surrealdb::rhai_engine::RhaiExpr>(&vantage_rhai::Env::new())
        })
        .unwrap();
    get_db()
        .await
        .execute(&SurrealReturn::new(e.0).expr())
        .await
        .unwrap()
}

/// [`read_rhai`] as text, `None` for any other value.
#[cfg(feature = "rhai")]
async fn read_text(code: &str) -> Option<String> {
    read_rhai(code).await.try_get::<String>()
}

/// `surreal::concat` and `concat_ws` take parts as `sql::concat` does: a
/// string is a bound value, never SurrealQL; `concat_ws` skips NONE and NULL.
#[cfg(feature = "rhai")]
#[tokio::test]
async fn rhai_surreal_concat_joins_parts() {
    let joined = read_text(r#"surreal::concat([surreal::expr("7"), ":", 5])"#).await;
    assert_eq!(joined.as_deref(), Some("7:5"));

    let typed = read_text(r#"surreal::concat([surreal::expr("7"), 5, ":", 1.5, ":", true])"#).await;
    assert_eq!(typed.as_deref(), Some("75:1.5:true"));

    // NONE itself (CBOR tag 6), not NULL, `false` or any other non-text.
    for missing in ["NONE", "NULL"] {
        let code =
            format!(r#"surreal::concat([surreal::expr("7"), ":", surreal::expr("{missing}")])"#);
        let value = read_rhai(&code).await;
        assert!(
            matches!(value.value(), ciborium::Value::Tag(6, _)),
            "a {missing} part makes concat NONE, as NULL does on SQL: {value:?}"
        );
    }

    let crafted = read_text(r#"surreal::concat(["\"; REMOVE TABLE x; --"])"#).await;
    assert_eq!(crafted.as_deref(), Some("\"; REMOVE TABLE x; --"));

    let ws = read_text(
        r#"surreal::concat_ws(", ", ["Riga", surreal::expr("NONE"), surreal::expr("NULL"), 7])"#,
    )
    .await;
    assert_eq!(ws.as_deref(), Some("Riga, 7"));
}
