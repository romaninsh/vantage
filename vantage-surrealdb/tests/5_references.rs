//! Test 5: Relationship traversal — with_one, with_many, get_ref_as.
//!
//! Uses the pre-populated SurrealDB v2 bakery database.
//! Requires: `cd scripts && ./ingress.sh` to populate.

use bakery_model3::{Bakery, Client, Product, SurrealConnection, SurrealDB};
use vantage_dataset::ReadableDataSet;
use vantage_surrealdb::surreal_expr;

async fn get_db() -> SurrealDB {
    let client = SurrealConnection::dsn("cbor://root:root@localhost:8000/bakery/v2")
        .expect("Invalid DSN")
        .connect()
        .await
        .expect("Failed to connect to SurrealDB — run scripts/ingress.sh first");
    SurrealDB::new(client)
}

/// Traverse has_many: bakery → products
#[tokio::test]
async fn test_has_many_products_for_bakery() {
    let db = get_db().await;
    let bakery = Bakery::surreal_table(db.clone());

    let products = bakery.get_ref_as::<SurrealDB, Product>("products").unwrap();

    let product_list = products.list().await.unwrap();
    // v2 has 5 products all belonging to hill_valley bakery
    assert_eq!(product_list.len(), 5);
}

/// Traverse has_one: product → bakery
#[tokio::test]
async fn test_has_one_bakery_for_product() {
    let db = get_db().await;
    let mut products = Product::surreal_table(db.clone());
    products.add_condition(surreal_expr!("name = {}", "Flux Capacitor Cupcake"));

    let bakery = products.get_ref_as::<SurrealDB, Bakery>("bakery").unwrap();

    let bakery_list = bakery.list().await.unwrap();
    assert_eq!(bakery_list.len(), 1);
    assert_eq!(
        bakery_list.values().next().unwrap().name,
        "Hill Valley Bakery"
    );
}

/// Traverse has_one: client → bakery (with condition)
#[tokio::test]
async fn test_has_one_bakery_for_paying_clients() {
    let db = get_db().await;
    let mut clients = Client::surreal_table(db.clone());
    clients.add_condition(surreal_expr!("is_paying_client = {}", true));

    let bakery = clients.get_ref_as::<SurrealDB, Bakery>("bakery").unwrap();

    let bakery_list = bakery.list().await.unwrap();
    // Both paying clients (Marty, Doc) belong to the same bakery
    assert_eq!(bakery_list.len(), 1);
    assert_eq!(
        bakery_list.values().next().unwrap().name,
        "Hill Valley Bakery"
    );
}
