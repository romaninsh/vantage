//! Test 4: ReadableDataSet for Table<SqliteDB, Entity>.
//!
//! Tests list, get, and get_some with entity deserialization against bakery.sqlite.

#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::ReadableDataSet;

const DB_PATH: &str = "sqlite:../target/bakery.sqlite?mode=ro";

async fn get_db() -> SqliteDB {
    SqliteDB::connect(DB_PATH)
        .await
        .expect("Failed to connect to bakery.sqlite — run scripts/sqlite/ingress.sh first")
}

#[entity(SqliteType)]
#[derive(Debug, Clone, PartialEq, Default)]
struct Product {
    name: String,
    calories: i64,
    price: i64,
    bakery_id: String,
    is_deleted: bool,
    inventory_stock: i64,
}

impl Product {
    fn sqlite_table(db: SqliteDB) -> Table<SqliteDB, Product> {
        Table::new("product", db)
            .with_id_column("id")
            .with_column_of::<String>("name")
            .with_column_of::<i64>("calories")
            .with_column_of::<i64>("price")
            .with_column_of::<String>("bakery_id")
            .with_column_of::<bool>("is_deleted")
            .with_column_of::<i64>("inventory_stock")
    }
}

#[tokio::test]
async fn test_list_products() {
    let db = get_db().await;
    let table = Product::sqlite_table(db);

    let products = table.list().await.unwrap();
    assert_eq!(products.len(), 5);

    let cupcake = &products["flux_cupcake"];
    assert_eq!(cupcake.name, "Flux Capacitor Cupcake");
    assert_eq!(cupcake.price, 120);
    assert_eq!(cupcake.calories, 300);
    assert!(!cupcake.is_deleted);
}

#[tokio::test]
async fn test_get_product() {
    let db = get_db().await;
    let table = Product::sqlite_table(db);

    let product = table
        .get("delorean_donut")
        .await
        .unwrap()
        .expect("delorean_donut exists");
    assert_eq!(product.name, "DeLorean Doughnut");
    assert_eq!(product.price, 135);
    assert_eq!(product.calories, 250);
}

#[tokio::test]
async fn test_get_some_product() {
    let db = get_db().await;
    let table = Product::sqlite_table(db);

    let result = table.get_some().await.unwrap();
    assert!(result.is_some());

    let (id, product) = result.unwrap();
    assert!(!id.is_empty());
    assert!(!product.name.is_empty());
    assert!(product.price > 0);
}
