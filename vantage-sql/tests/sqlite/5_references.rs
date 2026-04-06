//! Test 5: Relationship traversal — with_one, with_many, get_ref_as.
//!
//! Uses the pre-populated bakery.sqlite database.

#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_sql::sqlite_expr;
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::ReadableDataSet;

const DB_PATH: &str = "sqlite:../target/bakery.sqlite?mode=ro";

async fn get_db() -> SqliteDB {
    SqliteDB::connect(DB_PATH)
        .await
        .expect("Failed to connect to bakery.sqlite — run scripts/sqlite/ingress.sh first")
}

// ── Entity definitions ─────────────────────────────────────────────────────

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Client {
    name: String,
    email: String,
    contact_details: String,
    is_paying_client: bool,
    bakery_id: String,
}

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ClientOrder {
    bakery_id: String,
    client_id: String,
    is_deleted: bool,
}

// ── Table constructors with relationships ──────────────────────────────────

fn client_table(db: SqliteDB) -> Table<SqliteDB, Client> {
    let db2 = db.clone();
    Table::new("client", db)
        .with_id_column("id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("email")
        .with_column_of::<String>("contact_details")
        .with_column_of::<bool>("is_paying_client")
        .with_column_of::<String>("bakery_id")
        .with_many("orders", "client_id", move || order_table(db2.clone()))
}

fn order_table(db: SqliteDB) -> Table<SqliteDB, ClientOrder> {
    let db2 = db.clone();
    Table::new("client_order", db)
        .with_id_column("id")
        .with_column_of::<String>("bakery_id")
        .with_column_of::<String>("client_id")
        .with_column_of::<bool>("is_deleted")
        .with_one("client", "client_id", move || client_table(db2.clone()))
}

// ── Tests ──────────────────────────────────────────────────────────────────

/// Traverse has_many: paying clients → their orders
#[tokio::test]
async fn test_has_many_orders_for_paying_clients() {
    let db = get_db().await;
    let mut clients = client_table(db.clone());
    clients.add_condition(sqlite_expr!("{} = {}", (clients["is_paying_client"]), true));

    let orders = clients
        .get_ref_as::<SqliteDB, ClientOrder>("orders")
        .unwrap();

    let preview = orders.select().preview();
    assert_eq!(
        preview,
        "SELECT \"id\", \"bakery_id\", \"client_id\", \"is_deleted\" FROM \"client_order\" \
         WHERE client_id IN (SELECT \"id\" FROM \"client\" WHERE is_paying_client = 1)"
    );

    let order_list = orders.list().await.unwrap();
    // Marty has 1 order, Doc has 2 orders → 3 total
    assert_eq!(order_list.len(), 3);
}

/// Traverse has_many: single client → orders
#[tokio::test]
async fn test_has_many_orders_for_single_client() {
    let db = get_db().await;
    let mut clients = client_table(db.clone());
    clients.add_condition(sqlite_expr!("{} = {}", (clients["name"]), "Doc Brown"));

    let orders = clients
        .get_ref_as::<SqliteDB, ClientOrder>("orders")
        .unwrap();

    let preview = orders.select().preview();
    assert_eq!(
        preview,
        "SELECT \"id\", \"bakery_id\", \"client_id\", \"is_deleted\" FROM \"client_order\" \
         WHERE client_id IN (SELECT \"id\" FROM \"client\" WHERE name = 'Doc Brown')"
    );

    let order_list = orders.list().await.unwrap();
    assert_eq!(order_list.len(), 2);
}

/// Traverse has_one: order → client
#[tokio::test]
async fn test_has_one_client_for_order() {
    let db = get_db().await;
    let mut orders = order_table(db.clone());
    orders.add_condition(sqlite_expr!("{} = {}", (orders["id"]), "order1"));

    let client = orders
        .get_ref_as::<SqliteDB, Client>("client")
        .unwrap();

    let preview = client.select().preview();
    assert_eq!(
        preview,
        "SELECT \"id\", \"name\", \"email\", \"contact_details\", \"is_paying_client\", \"bakery_id\" \
         FROM \"client\" WHERE id IN (SELECT \"client_id\" FROM \"client_order\" WHERE id = 'order1')"
    );

    let client_list = client.list().await.unwrap();
    assert_eq!(client_list.len(), 1);
    assert_eq!(client_list.values().next().unwrap().name, "Marty McFly");
}
