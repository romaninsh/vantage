//! Test 4: Aggregates — COUNT, SUM, MAX, MIN via TableSource against seeded v2 data.

use vantage_mongodb::MongoDB;
use vantage_table::table::Table;
use vantage_table::traits::table_source::TableSource;
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
async fn test_count() {
    let db = get_db().await;
    let table = product_table(db);
    assert_eq!(table.get_count().await.unwrap(), 5);
}

#[tokio::test]
async fn test_max_price() {
    let db = get_db().await;
    let table = product_table(db.clone());
    let max = db.get_table_max(&table, &table["price"]).await.unwrap();
    assert_eq!(max.try_get::<i64>().unwrap(), 299);
}

#[tokio::test]
async fn test_min_price() {
    let db = get_db().await;
    let table = product_table(db.clone());
    let min = db.get_table_min(&table, &table["price"]).await.unwrap();
    assert_eq!(min.try_get::<i64>().unwrap(), 120);
}

#[tokio::test]
async fn test_sum_price() {
    let db = get_db().await;
    let table = product_table(db.clone());
    let sum = db.get_table_sum(&table, &table["price"]).await.unwrap();
    // 120 + 135 + 220 + 299 + 199 = 973
    assert_eq!(sum.try_get::<i64>().unwrap(), 973);
}
