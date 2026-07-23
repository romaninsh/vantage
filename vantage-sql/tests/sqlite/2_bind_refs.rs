//! Binding record references and malformed values.
//!
//! Reference dropdowns (and SurrealDB-shaped data generally) deliver
//! record references as `Tag(8, ...)` CBOR — either `["table", id]` or
//! `"table:id"`. SQLite has no reference type, so the binder lowers the
//! reference to its id. A value the binder cannot lower must surface as
//! an error that names the parameter — never a panic: these values come
//! straight from user forms.

use vantage_expressions::{ExprDataSource, Expression, ExpressiveEnum};
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_types::Record;

use ciborium::Value as CborValue;

async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();

    sqlx::query(
        "CREATE TABLE book (
            id INTEGER PRIMARY KEY,
            title TEXT NOT NULL,
            author_id INTEGER NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(
        "CREATE TABLE note (
            id INTEGER PRIMARY KEY,
            owner TEXT NOT NULL
        )",
    )
    .execute(db.pool())
    .await
    .unwrap();

    db
}

/// An expression whose parameters are raw CBOR without type variants —
/// the shape `insert_table_value` produces from record data.
fn untyped_expr(template: &str, params: Vec<CborValue>) -> Expression<AnySqliteType> {
    Expression::new(
        template,
        params
            .into_iter()
            .map(|v| ExpressiveEnum::Scalar(AnySqliteType::untyped(v)))
            .collect(),
    )
}

fn records(result: AnySqliteType) -> Vec<Record<AnySqliteType>> {
    Vec::<Record<AnySqliteType>>::try_from(result).unwrap()
}

fn reference_array(table: &str, id: CborValue) -> CborValue {
    CborValue::Tag(
        8,
        Box::new(CborValue::Array(vec![
            CborValue::Text(table.to_string()),
            id,
        ])),
    )
}

// ── Tag(8) record references lower to their id ─────────────────────────────

#[tokio::test]
async fn reference_with_text_id_lowers_into_integer_column() {
    let db = setup().await;

    // The exact shape from the field report: Tag(8, ["authors", "1"]).
    let insert = untyped_expr(
        "INSERT INTO \"book\" (\"title\", \"author_id\") VALUES ({}, {})",
        vec![
            CborValue::Text("Kindred".into()),
            reference_array("authors", CborValue::Text("1".into())),
        ],
    );
    db.execute(&insert).await.unwrap();

    let select = untyped_expr(
        "SELECT author_id FROM book WHERE title = {}",
        vec![CborValue::Text("Kindred".into())],
    );
    let rows = records(db.execute(&select).await.unwrap());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["author_id"].try_get::<i64>(), Some(1));
}

#[tokio::test]
async fn reference_with_integer_id_lowers_into_integer_column() {
    let db = setup().await;

    let insert = untyped_expr(
        "INSERT INTO \"book\" (\"title\", \"author_id\") VALUES ({}, {})",
        vec![
            CborValue::Text("Dawn".into()),
            reference_array("authors", CborValue::Integer(3.into())),
        ],
    );
    db.execute(&insert).await.unwrap();

    let select = untyped_expr(
        "SELECT author_id FROM book WHERE title = {}",
        vec![CborValue::Text("Dawn".into())],
    );
    let rows = records(db.execute(&select).await.unwrap());
    assert_eq!(rows[0]["author_id"].try_get::<i64>(), Some(3));
}

#[tokio::test]
async fn reference_in_string_form_lowers_to_id_part() {
    let db = setup().await;

    // SurrealDB string form: Tag(8, "authors:2").
    let insert = untyped_expr(
        "INSERT INTO \"book\" (\"title\", \"author_id\") VALUES ({}, {})",
        vec![
            CborValue::Text("Wild Seed".into()),
            CborValue::Tag(8, Box::new(CborValue::Text("authors:2".into()))),
        ],
    );
    db.execute(&insert).await.unwrap();

    let select = untyped_expr(
        "SELECT author_id FROM book WHERE title = {}",
        vec![CborValue::Text("Wild Seed".into())],
    );
    let rows = records(db.execute(&select).await.unwrap());
    assert_eq!(rows[0]["author_id"].try_get::<i64>(), Some(2));
}

#[tokio::test]
async fn reference_with_non_numeric_id_lands_in_text_column() {
    let db = setup().await;

    let insert = untyped_expr(
        "INSERT INTO \"note\" (\"owner\") VALUES ({})",
        vec![reference_array("user", CborValue::Text("gita".into()))],
    );
    db.execute(&insert).await.unwrap();

    let select = untyped_expr("SELECT owner FROM note", vec![]);
    let rows = records(db.execute(&select).await.unwrap());
    assert_eq!(rows[0]["owner"].try_get::<String>(), Some("gita".into()));
}

// ── Unbindable values error with a named parameter — never panic ───────────

#[tokio::test]
async fn unknown_tag_errors_instead_of_panicking() {
    let db = setup().await;

    let insert = untyped_expr(
        "INSERT INTO \"note\" (\"owner\") VALUES ({})",
        vec![CborValue::Tag(999, Box::new(CborValue::Array(vec![])))],
    );
    let err = db.execute(&insert).await.unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("parameter"),
        "should name the parameter: {msg}"
    );
    assert!(msg.contains("999"), "should describe the value: {msg}");
}

#[tokio::test]
async fn bind_error_names_parameter_position() {
    let db = setup().await;

    // Second parameter is the broken one — the error must say so.
    let insert = untyped_expr(
        "INSERT INTO \"book\" (\"title\", \"author_id\") VALUES ({}, {})",
        vec![CborValue::Text("ok".into()), CborValue::Map(vec![])],
    );
    let err = db.execute(&insert).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains('2'), "should name parameter 2: {msg}");
}

// ── Failed queries carry the SQL and a parameter-type summary ──────────────

#[tokio::test]
async fn failed_query_error_includes_sql_and_param_types() {
    let db = setup().await;

    // A non-numeric text bound to the INTEGER PRIMARY KEY (rowid) column
    // is the classic SQLITE_MISMATCH — the id-strategy failure shape.
    let insert = untyped_expr(
        "INSERT INTO \"book\" (\"id\", \"title\", \"author_id\") VALUES ({}, {}, {})",
        vec![
            CborValue::Text("0198c5d2-not-a-rowid".into()),
            CborValue::Text("Kindred".into()),
            CborValue::Integer(1.into()),
        ],
    );
    let err = db.execute(&insert).await.unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("INSERT INTO \\\"book\\\"") || msg.contains("INSERT INTO \"book\""),
        "error should carry the SQL: {msg}"
    );
    assert!(
        msg.contains("Text") && msg.contains("Integer"),
        "error should summarize parameter types: {msg}"
    );
}
