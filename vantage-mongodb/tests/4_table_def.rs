//! Test 4: Table definition and query generation via TableSource.

use vantage_mongodb::MongoDB;
use vantage_table::table::Table;
use vantage_types::EmptyEntity;

fn mongo_url() -> String {
    std::env::var("MONGODB_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into())
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
async fn test_product_select() {
    let db = MongoDB::connect(&mongo_url(), "vantage").await.unwrap();
    let table = product_table(db);
    let select = table.select();

    assert_eq!(select.collection, Some("product".to_string()));
    assert_eq!(
        select.fields,
        vec![
            "_id",
            "name",
            "calories",
            "price",
            "bakery_id",
            "is_deleted",
            "inventory_stock"
        ]
    );
    assert!(select.preview().starts_with("db.product.find({})"));
}
