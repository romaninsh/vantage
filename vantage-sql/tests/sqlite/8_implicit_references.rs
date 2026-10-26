//! Test 8: Implicit references — dotted active columns (VAN-102).
//!
//! A dotted name in `with_active_columns` traverses declared `has_one`
//! relations and imports the target's field as a read-only column, lowered
//! into a nested correlated scalar subquery. Uses a self-contained in-memory
//! database so the write-strip case can insert.

use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
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

/// `client_order -> client.name` (one hop) and `client.bakery.name` (two hops)
/// are lowered into correlated subqueries, the second nested in the first, and
/// aliased under the flat dotted keys.
#[tokio::test]
async fn dotted_columns_project_correlated_subqueries() {
    let db = setup().await;
    let orders = order_table(db)
        .with_active_columns(&["id", "client_id", "client.name", "client.bakery.name"])
        .unwrap();

    let preview = orders.select().preview();
    assert!(preview.contains("client.name"), "alias missing: {preview}");
    assert!(
        preview.contains("client.bakery.name"),
        "alias missing: {preview}"
    );
    assert!(preview.contains("client_id"), "base col missing: {preview}");
    // A correlated subquery over the client table, not a join.
    assert!(
        preview.contains(r#""client"."id" = "client_order"."client_id""#),
        "no correlated subquery: {preview}"
    );

    let rows = orders.list_values().await.unwrap();
    assert_eq!(rows.len(), 2);
    let o1 = &rows["o1"];
    assert_eq!(
        o1.get("client.name").map(ToString::to_string),
        Some("'Marty'".to_string())
    );
    assert_eq!(
        o1.get("client.bakery.name").map(ToString::to_string),
        Some("'Sunrise Bakery'".to_string())
    );
}

/// An imported column exists only as a projection alias, so a vista
/// condition on it is refused, never sent as a column name.
#[cfg(feature = "vista")]
#[tokio::test]
async fn vista_condition_on_an_imported_column_is_refused() {
    use vantage_vista::FilterOp;

    let db = setup().await;
    let orders = order_table(db.clone())
        .with_active_columns(&["id", "client_id", "client.name"])
        .unwrap();
    let vista = db.vista_factory().from_table(orders).unwrap();
    assert!(vista.column("client.name").is_some(), "imported column");

    // The column is only a projection alias, so SQL refuses it and a caller
    // evaluates it over the rows it reads.
    let err = vista
        .clone()
        .with_condition("client.name", FilterOp::Eq, "Marty")
        .unwrap_err();
    assert!(err.is_unimplemented(), "{err:?}");
    let condition = vantage_vista::Condition::new("client.name", FilterOp::Eq, "Marty");
    let query = vantage_vista::VistaQuery::new().with_condition(condition.clone());
    let (narrowed, residual) = vista.with_query(&query).unwrap();
    assert_eq!(residual.conditions, vec![condition]);
    assert!(narrowed.query().conditions.is_empty());
}

/// A `has_many` hop, an unknown relation and an unknown target column are
/// build-time errors, not wrong queries.
#[tokio::test]
async fn bad_dotted_columns_error_at_build() {
    let db = setup().await;
    // `orders` on client is has_many.
    let has_many = client_table(db.clone())
        .with_active_columns(&["orders.is_deleted"])
        .expect_err("a has_many hop must fail")
        .to_string();
    let unknown_relation = order_table(db.clone())
        .with_active_columns(&["nope.name"])
        .expect_err("an unknown relation must fail")
        .to_string();
    let unknown_column = order_table(db)
        .with_active_columns(&["client.does_not_exist"])
        .expect_err("an unknown target column must fail")
        .to_string();
    assert_ne!(has_many, unknown_relation);
    assert_ne!(has_many, unknown_column);
    assert_ne!(unknown_relation, unknown_column);
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

/// The generated-id insert path (`insert_return_id_value`, which the typed
/// entity insert funnels through) must strip imported columns just like the
/// explicit-id path — otherwise a round-tripped record fails the INSERT.
#[tokio::test]
async fn imported_columns_are_stripped_on_insert_return_id() {
    let db = setup().await;
    let orders = order_table(db)
        .with_active_columns(&["id", "client_id", "client.name"])
        .unwrap();

    let mut rec: Record<AnySqliteType> = Record::new();
    // Explicit id: the TEXT primary key has no DEFAULT, so a generated id
    // would come back NULL — irrelevant to what this test pins down.
    rec.insert("id".to_string(), AnySqliteType::from("o9".to_string()));
    rec.insert(
        "client_id".to_string(),
        AnySqliteType::from("c1".to_string()),
    );
    rec.insert(
        "client.name".to_string(),
        AnySqliteType::from("should be dropped".to_string()),
    );

    // Succeeds only if the imported column is stripped on this path too.
    let id = orders.insert_return_id_value(&rec).await.unwrap();
    let stored = orders.get_value(id).await.unwrap().unwrap();
    assert_eq!(
        stored.get("client.name").map(ToString::to_string),
        Some("'Marty'".to_string())
    );
}

/// A patch is explicit intent per key: patching a read-only imported column
/// is rejected loudly instead of silently no-opping.
#[tokio::test]
async fn patch_on_imported_column_errors() {
    let db = setup().await;
    let orders = order_table(db)
        .with_active_columns(&["id", "client_id", "client.name"])
        .unwrap();

    let mut patch: Record<AnySqliteType> = Record::new();
    patch.insert(
        "client.name".to_string(),
        AnySqliteType::from("nope".to_string()),
    );
    let err = orders.patch_value("o1", &patch).await.unwrap_err();
    assert!(
        err.to_string().contains("read-only"),
        "unexpected error: {err}"
    );

    // Sanity: patching a real column still works.
    let mut ok_patch: Record<AnySqliteType> = Record::new();
    ok_patch.insert(
        "client_id".to_string(),
        AnySqliteType::from("c1".to_string()),
    );
    orders.patch_value("o1", &ok_patch).await.unwrap();
}

/// The active set restricts projection: a declared column left out of the set
/// is absent from the query and the rows. The id column is projected anyway —
/// consumers key rows by it.
#[tokio::test]
async fn active_set_restricts_projection_but_keeps_id() {
    let db = setup().await;
    let orders = order_table(db).with_active_columns(&["client_id"]).unwrap();

    let preview = orders.select().preview();
    assert!(
        !preview.contains("is_deleted"),
        "inactive column projected: {preview}"
    );

    let rows = orders.list_values().await.unwrap();
    assert_eq!(rows.len(), 2);
    let o1 = &rows["o1"];
    assert!(o1.get("id").is_some(), "id missing from row: {o1:?}");
    assert!(o1.get("client_id").is_some());
    assert!(o1.get("is_deleted").is_none());
}

/// A column registered only as an expression (no column def) can be named in
/// the active set and stays projected.
#[tokio::test]
async fn expression_only_column_can_be_activated() {
    let db = setup().await;
    let orders = order_table(db)
        .with_expression("client_upper", |_| sqlite_expr!("UPPER(client_id)"))
        .with_active_columns(&["id", "client_upper"])
        .unwrap();

    let preview = orders.select().preview();
    assert!(
        preview.contains("client_upper"),
        "expression column dropped: {preview}"
    );

    let rows = orders.list_values().await.unwrap();
    for row in rows.values() {
        assert_eq!(
            row.get("client_upper").map(ToString::to_string),
            Some("'C1'".to_string())
        );
    }
}
