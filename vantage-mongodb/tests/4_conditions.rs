//! Test 4: Conditions on Table<MongoDB, EmptyEntity> against seeded v2 data.

use bson::doc;
use vantage_dataset::prelude::*;
use vantage_mongodb::{MongoDB, MongoId};
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
}

async fn get_db() -> MongoDB {
    MongoDB::connect(&mongo_url(), "vantage").await.unwrap()
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
async fn test_condition_gt() {
    let db = get_db().await;
    let mut table = product_table(db);
    table.add_condition(doc! { "price": { "$gt": 130 } });

    let products = table.list_values().await.unwrap();
    assert_eq!(products.len(), 4); // 135, 220, 299, 199
}

#[tokio::test]
async fn test_multiple_conditions() {
    let db = get_db().await;
    let mut table = product_table(db);
    table.add_condition(doc! { "price": { "$gt": 130 } });
    table.add_condition(doc! { "$expr": { "$gt": ["$price", "$calories"] } });

    let products = table.list_values().await.unwrap();
    // price > 130 AND price > calories:
    // delorean_donut: 135 > 250? no
    // time_tart: 220 > 200? yes
    // sea_pie: 299 > 350? no
    // hover_cookies: 199 > 150? yes
    assert_eq!(products.len(), 2);
    assert!(products.contains_key(&MongoId::from("time_tart")));
    assert!(products.contains_key(&MongoId::from("hover_cookies")));
}

#[tokio::test]
async fn test_condition_eq_bool() {
    let db = get_db().await;
    let mut table = product_table(db);
    table.add_condition(doc! { "is_deleted": false });

    let products = table.list_values().await.unwrap();
    assert_eq!(products.len(), 5); // all products have is_deleted=false
}
