//! Test 8: Implicit references — dotted active columns (VAN-102).
//!
//! A dotted name in `with_active_columns` traverses declared `has_one`
//! relations and imports the target's field as a read-only column, lowered
//! into a nested correlated scalar subquery. Uses a self-contained in-memory
//! database so the write-strip case can insert.

use vantage_dataset::traits::{ReadableValueSet, WritableValueSet};
use vantage_expressions::ExprDataSource;
#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_sql::sqlite_expr;
use vantage_table::table::Table;
use vantage_types::{Record, entity};

async fn setup() -> SqliteDB {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    for ddl in [
        "CREATE TABLE bakery (id TEXT PRIMARY KEY, name TEXT NOT NULL)",
        "CREATE TABLE client (id TEXT PRIMARY KEY, name TEXT NOT NULL, bakery_id TEXT)",
        "CREATE TABLE client_order (id TEXT PRIMARY KEY, client_id TEXT, is_deleted BOOLEAN NOT NULL DEFAULT 0)",
    ] {
        sqlx::query(ddl).execute(db.pool()).await.unwrap();
    }
    db.execute(&sqlite_expr!(
        "INSERT INTO bakery (id, name) VALUES ({}, {})",
        "b1",
        "Sunrise Bakery"
    ))
    .await
    .unwrap();
    db.execute(&sqlite_expr!(
        "INSERT INTO client (id, name, bakery_id) VALUES ({}, {}, {})",
        "c1",
        "Marty",
        "b1"
    ))
    .await
    .unwrap();
    db.execute(&sqlite_expr!(
        "INSERT INTO client_order (id, client_id) VALUES ({}, {}), ({}, {})",
        "o1",
        "c1",
        "o2",
        "c1"
    ))
    .await
    .unwrap();
    db
}

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Bakery {
    name: String,
}

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Client {
    name: String,
    bakery_id: String,
}

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ClientOrder {
    client_id: String,
    is_deleted: bool,
}

fn bakery_table(db: SqliteDB) -> Table<SqliteDB, Bakery> {
    Table::new("bakery", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
}

fn client_table(db: SqliteDB) -> Table<SqliteDB, Client> {
    Table::new("client", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("bakery_id")
        .with_one("bakery", "bakery_id", bakery_table)
        .with_many("orders", "client_id", order_table)
}

fn order_table(db: SqliteDB) -> Table<SqliteDB, ClientOrder> {
    Table::new("client_order", db)
        .with_id_column("id")
        .with_column_of::<String>("client_id")
        .with_column_of::<bool>("is_deleted")
        .with_one("client", "client_id", client_table)
}

/// One hop: `client_order -> client.name`, lowered into a correlated subquery
/// and aliased under the flat dotted key. Preview shows the shape; the live
/// rows prove the correlation resolves.
#[tokio::test]
async fn one_hop_projects_correlated_subquery() {
    let db = setup().await;
    let orders = order_table(db)
        .with_active_columns(&["id", "client_id", "client.name"])
        .unwrap();

    let preview = orders.select().preview();
    assert!(preview.contains("client.name"), "alias missing: {preview}");
    assert!(preview.contains("client_id"), "base col missing: {preview}");
    // A correlated subquery over the client table, not a join.
    assert!(preview.contains("client"), "no client ref: {preview}");

    let rows = orders.list_values().await.unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows.values() {
        assert_eq!(
            row.get("client.name").map(ToString::to_string),
            Some("'Marty'".to_string())
        );
    }
}

/// Two hops: `client_order -> client -> bakery.name` nests one correlated
/// subquery inside another.
#[tokio::test]
async fn two_hop_nests_subqueries() {
    let db = setup().await;
    let orders = order_table(db)
        .with_active_columns(&["id", "client.name", "client.bakery.name"])
        .unwrap();

    let preview = orders.select().preview();
    assert!(
        preview.contains("client.bakery.name"),
        "alias missing: {preview}"
    );
    assert!(preview.contains("bakery"), "no bakery ref: {preview}");

    let rows = orders.list_values().await.unwrap();
    assert_eq!(rows.len(), 2);
    for row in rows.values() {
        assert_eq!(
            row.get("client.bakery.name").map(ToString::to_string),
            Some("'Sunrise Bakery'".to_string())
        );
    }
}

/// A `has_many` hop is a build-time error, not a wrong query.
#[tokio::test]
async fn has_many_hop_errors_at_build() {
    let db = setup().await;
    // `orders` on client is has_many.
    let result = client_table(db).with_active_columns(&["orders.is_deleted"]);
    assert!(result.is_err());
}

/// An unknown relation is a build-time error.
#[tokio::test]
async fn unknown_relation_errors_at_build() {
    let db = setup().await;
    let result = order_table(db).with_active_columns(&["nope.name"]);
    assert!(result.is_err());
}

/// An unknown column on the final target is a build-time error.
#[tokio::test]
async fn unknown_target_column_errors_at_build() {
    let db = setup().await;
    let result = order_table(db).with_active_columns(&["client.does_not_exist"]);
    assert!(result.is_err());
}

/// An imported dotted column never reaches a write: the strip drops it before
/// the INSERT, so a round-tripped record carrying `client.name` still inserts
/// (otherwise SQLite would reject the unknown column).
#[tokio::test]
async fn imported_columns_are_stripped_on_write() {
    let db = setup().await;
    let orders = order_table(db)
        .with_active_columns(&["id", "client_id", "client.name"])
        .unwrap();

    let mut rec: Record<AnySqliteType> = Record::new();
    rec.insert(
        "client_id".to_string(),
        AnySqliteType::from("c1".to_string()),
    );
    rec.insert(
        "client.name".to_string(),
        AnySqliteType::from("should be dropped".to_string()),
    );

    // Succeeds only because the imported column was stripped before the INSERT;
    // otherwise SQLite rejects the unknown `client.name` column.
    orders.insert_value("o3", &rec).await.unwrap();

    let stored = orders.get_value("o3".to_string()).await.unwrap().unwrap();
    // The real column survived…
    assert_eq!(
        stored.get("client_id").map(ToString::to_string),
        Some("'c1'".to_string())
    );
    // …and `client.name` reads back as the correlated value (Marty), never the
    // literal "should be dropped" that the write attempted to smuggle in.
    assert_eq!(
        stored.get("client.name").map(ToString::to_string),
        Some("'Marty'".to_string())
    );
}
