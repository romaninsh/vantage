//! Test 5: Relationship traversal — `with_many` + `get_ref_as` on Redb.
//!
//! Tests exercise `related_in_condition`, which builds a deferred
//! condition over the target table's foreign-key column. That column
//! has to be flagged `Indexed` so the resolved IN lookup can hit a
//! real index.

use vantage_dataset::prelude::*;
use vantage_redb::operation::RedbOperation;
use vantage_redb::{AnyRedbType, Redb};
use vantage_table::column::core::Column;
use vantage_table::column::flags::ColumnFlag;
use vantage_table::table::Table;
use vantage_types::entity;

#[entity(RedbType)]
#[derive(Clone, Debug, Default, PartialEq)]
struct Client {
    name: String,
    is_paying: bool,
}

#[entity(RedbType)]
#[derive(Clone, Debug, Default, PartialEq)]
struct Order {
    client_id: String,
    total: i64,
}

fn client_table(db: Redb) -> Table<Redb, Client> {
    Table::<Redb, Client>::new("client", db)
        .with_id_column("id")
        .with_column(Column::<bool>::new("is_paying").with_flag(ColumnFlag::Indexed))
        .with_column_of::<String>("name")
        .with_many("orders", "client_id", order_table)
}

fn order_table(db: Redb) -> Table<Redb, Order> {
    Table::<Redb, Order>::new("order", db)
        .with_id_column("id")
        .with_column(Column::<String>::new("client_id").with_flag(ColumnFlag::Indexed))
        .with_column_of::<i64>("total")
        .with_one("client", "client_id", client_table)
}

async fn seed() -> (tempfile::NamedTempFile, Redb) {
    let path = tempfile::NamedTempFile::new().unwrap();
    let db = Redb::create(path.path()).unwrap();

    let clients = client_table(db.clone());
    for (id, name, is_paying) in [
        ("marty", "Marty McFly", true),
        ("doc", "Doc Brown", true),
        ("biff", "Biff Tannen", false),
    ] {
        clients
            .insert(
                &id.to_string(),
                &Client {
                    name: name.into(),
                    is_paying,
                },
            )
            .await
            .unwrap();
    }

    let orders = order_table(db.clone());
    for (id, client_id, total) in [
        ("o1", "marty", 100),
        ("o2", "doc", 200),
        ("o3", "doc", 50),
        ("o4", "biff", 30),
    ] {
        orders
            .insert(
                &id.to_string(),
                &Order {
                    client_id: client_id.into(),
                    total,
                },
            )
            .await
            .unwrap();
    }

    (path, db)
}

#[tokio::test]
async fn test_has_many_orders_for_paying_clients() {
    let (_tmp, db) = seed().await;
    let mut clients = client_table(db);
    let is_paying = clients["is_paying"].clone();
    clients.add_condition(is_paying.eq(true));

    let orders = clients.get_ref_as::<Order>("orders").unwrap();
    let list = orders.list().await.unwrap();
    // Marty (1) + Doc (2) = 3 orders.
    assert_eq!(list.len(), 3);
    assert!(list.contains_key("o1"));
    assert!(list.contains_key("o2"));
    assert!(list.contains_key("o3"));
    assert!(!list.contains_key("o4"));
}

#[tokio::test]
async fn test_has_many_orders_for_single_client() {
    let (_tmp, db) = seed().await;

    let mut clients = client_table(db);
    let id_col = clients["id"].clone();
    clients.add_condition(id_col.eq("doc"));

    let orders = clients.get_ref_as::<Order>("orders").unwrap();
    let list = orders.list().await.unwrap();
    assert_eq!(list.len(), 2);
    assert!(list.contains_key("o2"));
    assert!(list.contains_key("o3"));
}

#[tokio::test]
async fn test_has_many_for_no_matching_clients_yields_empty() {
    let (_tmp, db) = seed().await;

    let mut clients = client_table(db);
    let id_col = clients["id"].clone();
    clients.add_condition(id_col.eq("nonexistent"));

    let orders = clients.get_ref_as::<Order>("orders").unwrap();
    let list = orders.list().await.unwrap();
    assert!(list.is_empty());
}

#[tokio::test]
async fn test_has_one_client_for_order() {
    let (_tmp, db) = seed().await;

    let mut orders = order_table(db);
    let id_col = orders["id"].clone();
    orders.add_condition(id_col.eq("o1"));

    let client = orders.get_ref_as::<Client>("client").unwrap();
    let list = client.list().await.unwrap();
    assert_eq!(list.len(), 1);
    assert!(list.contains_key("marty"));
}

#[tokio::test]
async fn test_typed_entity_round_trip() {
    let (_tmp, db) = seed().await;
    let table = client_table(db);

    // Reads through the typed `get` go entity → record → struct via the
    // generated TryFromRecord impl on Client.
    let marty: Client = table
        .get("marty".to_string())
        .await
        .unwrap()
        .expect("marty exists");
    assert_eq!(marty.name, "Marty McFly");
    assert!(marty.is_paying);
}
