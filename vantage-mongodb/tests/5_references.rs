//! Test 5: Relationship traversal — with_one, with_many, get_ref_as.
//!
//! Uses the seeded `vantage` database (scripts/db/v2.js).

use bson::doc;
use vantage_dataset::ReadableDataSet;
use vantage_dataset::traits::ReadableValueSet;
#[allow(unused_imports)]
use vantage_mongodb::AnyMongoType;
use vantage_mongodb::{MongoDB, MongoId};
use vantage_table::table::{RollupOp, Table};
use vantage_types::entity;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn get_db() -> MongoDB {
    MongoDB::connect(&mongo_url(), "vantage")
        .await
        .expect("Failed to connect to MongoDB")
}

#[entity(MongoType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Client {
    name: String,
    email: String,
    contact_details: String,
    is_paying_client: bool,
    bakery_id: String,
}

#[entity(MongoType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct ClientOrder {
    client_id: String,
    is_deleted: bool,
}

fn client_table(db: MongoDB) -> Table<MongoDB, Client> {
    Table::new("client", db)
        .with_id_column("_id")
        .with_column_of::<String>("name")
        .with_column_of::<String>("email")
        .with_column_of::<String>("contact_details")
        .with_column_of::<bool>("is_paying_client")
        .with_column_of::<String>("bakery_id")
        .with_many("orders", "client_id", order_table)
}

fn order_table(db: MongoDB) -> Table<MongoDB, ClientOrder> {
    Table::new("client_order", db)
        .with_id_column("_id")
        .with_column_of::<String>("client_id")
        .with_column_of::<bool>("is_deleted")
        .with_one("client", "client_id", client_table)
}

/// has_many: client doc -> his orders; has_one: order1 -> its client.
#[tokio::test]
async fn test_get_ref_as_traverses_has_many_and_has_one() {
    let db = get_db().await;
    let mut doc_brown = client_table(db.clone());
    doc_brown.add_condition(doc! { "_id": "doc" });
    let mut order1 = order_table(db.clone());
    order1.add_condition(doc! { "_id": "order1" });

    let orders = doc_brown
        .get_ref_as::<ClientOrder>("orders")
        .unwrap()
        .list()
        .await
        .unwrap();
    assert_eq!(orders.len(), 2);
    assert!(orders.contains_key(&MongoId::from("order2")), "{orders:?}");
    assert!(orders.contains_key(&MongoId::from("order3")), "{orders:?}");

    let client = order1
        .get_ref_as::<Client>("client")
        .unwrap()
        .list()
        .await
        .unwrap();
    assert_eq!(client.len(), 1);
    assert_eq!(client.values().next().unwrap().name, "Marty McFly");
}

/// A client that never connects: the calls below fail before any query.
async fn offline_db() -> MongoDB {
    let client = mongodb::Client::with_uri_str("mongodb://127.0.0.1:1")
        .await
        .unwrap();
    MongoDB::new(client, "offline")
}

/// Roll-ups and correlated subqueries are refused with an error, not a panic.
/// A roll-up's refusal comes from the check a factory runs on a finished
/// table and from the read, before anything is sent.
#[tokio::test]
async fn rollups_and_correlated_subqueries_error() {
    let clients = client_table(offline_db().await);

    let rolled = clients
        .clone()
        .with_rollup("orders", "orders", RollupOp::Count, None, None)
        .unwrap();
    for err in [
        rolled.check_expressions().unwrap_err(),
        rolled.list_values().await.unwrap_err(),
    ] {
        assert_eq!(
            err.message(),
            "rollup columns aren't supported on MongoDB yet"
        );
        assert_eq!(err.context["expression_column"], "orders");
        assert!(err.is_unsupported(), "{err:?}");
    }

    let err = clients
        .get_subquery_as::<ClientOrder>("orders")
        .unwrap_err();
    assert!(err.is_unsupported(), "{err:?}");
}
