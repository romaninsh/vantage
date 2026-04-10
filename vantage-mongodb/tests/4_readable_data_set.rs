//! Test 4: ReadableValueSet for Table<MongoDB, EmptyEntity> against seeded v2 data.

use vantage_dataset::prelude::*;
use vantage_mongodb::{MongoDB, MongoId};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn get_db() -> MongoDB {
    MongoDB::connect(&mongo_url(), "vantage")
        .await
        .expect("Failed to connect — run scripts/start.sh + scripts/ingress.sh first")
}

fn product_table(db: MongoDB) -> Table<MongoDB, EmptyEntity> {
    Table::new("product", db)
        .with_id_column("_id")
        .with_column_of::<String>("name")
        .with_column_of::<i64>("calories")
        .with_column_of::<i64>("price")
        .with_column_of::<String>("bakery_id")
        .with_column_of::<bool>("is_deleted")
        .with_column_of::<i64>("inventory_stock")
}

#[tokio::test]
async fn test_list_products() {
    let db = get_db().await;
    let table = product_table(db);

    let values = table.list_values().await.unwrap();
    assert_eq!(values.len(), 5);

    let cupcake = &values[&MongoId::from("flux_cupcake")];
    assert_eq!(
        cupcake["name"].try_get::<String>(),
        Some("Flux Capacitor Cupcake".into())
    );
    assert_eq!(cupcake["price"].try_get::<i64>(), Some(120));
    assert_eq!(cupcake["calories"].try_get::<i64>(), Some(300));
    assert_eq!(cupcake["is_deleted"].try_get::<bool>(), Some(false));
}

#[tokio::test]
async fn test_get_product() {
    let db = get_db().await;
    let table = product_table(db);

    let record = table
        .get_value(&MongoId::from("delorean_donut"))
        .await
        .unwrap();
    assert_eq!(
        record["name"].try_get::<String>(),
        Some("DeLorean Doughnut".into())
    );
    assert_eq!(record["price"].try_get::<i64>(), Some(135));
    assert_eq!(record["calories"].try_get::<i64>(), Some(250));
}

#[tokio::test]
async fn test_get_some_product() {
    let db = get_db().await;
    let table = product_table(db);

    let result = table.get_some_value().await.unwrap();
    assert!(result.is_some());

    let (id, record) = result.unwrap();
    assert!(!id.to_string().is_empty());
    assert!(record.get("name").is_some());
    assert!(record.get("price").is_some());
}
