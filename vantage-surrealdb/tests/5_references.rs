//! Test 5: Relationship traversal — with_one, with_many, get_ref_as.
//!
//! Uses the pre-populated SurrealDB v2 bakery database.
//! Requires: `cd scripts && ./ingress.sh` to populate.

use bakery_model3::{Bakery, Client, Product, SurrealConnection, SurrealDB};
use vantage_dataset::ReadableDataSet;
use vantage_surrealdb::surreal_expr;
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::types::AnySurrealType;
use vantage_types::Record;

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

    let products = bakery.get_ref_as::<Product>("products").unwrap();

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

    let bakery = products.get_ref_as::<Bakery>("bakery").unwrap();

    let bakery_list = bakery.list().await.unwrap();
    assert_eq!(bakery_list.len(), 1);
    assert_eq!(
        bakery_list.values().next().unwrap().name,
        "Hill Valley Bakery"
    );
}

/// Traverse has_many from a row in hand. The row's id may be a record id or
/// its `table:key` text, as a row that came through JSON or a script carries
/// it; both narrow to the same products.
#[tokio::test]
async fn test_has_many_from_a_row_takes_the_id_as_record_or_text() {
    let db = get_db().await;
    let bakery = Bakery::surreal_table(db.clone());

    for id in [
        AnySurrealType::new(Thing::new("bakery", "hill_valley")),
        AnySurrealType::new("bakery:hill_valley".to_string()),
    ] {
        let row: Record<AnySurrealType> = [("id".to_string(), id)].into_iter().collect();
        let products = bakery
            .get_ref_from_row::<Product>("products", &row)
            .unwrap();
        assert_eq!(products.list().await.unwrap().len(), 5);
    }
}

/// Traverse has_one: client → bakery (with condition)
#[tokio::test]
async fn test_has_one_bakery_for_paying_clients() {
    let db = get_db().await;
    let mut clients = Client::surreal_table(db.clone());
    clients.add_condition(surreal_expr!("is_paying_client = {}", true));

    let bakery = clients.get_ref_as::<Bakery>("bakery").unwrap();

    let bakery_list = bakery.list().await.unwrap();
    // Both paying clients (Marty, Doc) belong to the same bakery
    assert_eq!(bakery_list.len(), 1);
    assert_eq!(
        bakery_list.values().next().unwrap().name,
        "Hill Valley Bakery"
    );
}
