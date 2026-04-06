//! Test 4: ReadableValueSet for Table<SqliteDB, Entity>.
//!
//! Tests list_values, get_value, and get_some_value against bakery.sqlite.

#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::ReadableValueSet;

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
async fn test_list_values_products() {
    let db = get_db().await;
    let table = Product::sqlite_table(db);

    let values = table.list_values().await.unwrap();
    assert_eq!(values.len(), 5);

    assert!(values.contains_key("flux_cupcake"));
    assert!(values.contains_key("delorean_donut"));

    let cupcake = &values["flux_cupcake"];
    assert_eq!(
        cupcake["name"].try_get::<String>(),
        Some("Flux Capacitor Cupcake".to_string())
    );
    assert_eq!(cupcake["price"].try_get::<i64>(), Some(120));
}

#[tokio::test]
async fn test_get_value_product() {
    let db = get_db().await;
    let table = Product::sqlite_table(db);

    let record = table
        .get_value(&"delorean_donut".to_string())
        .await
        .unwrap();
    assert_eq!(
        record["name"].try_get::<String>(),
        Some("DeLorean Doughnut".to_string())
    );
    assert_eq!(record["price"].try_get::<i64>(), Some(135));
    assert_eq!(record["calories"].try_get::<i64>(), Some(250));
}

#[tokio::test]
async fn test_get_some_value_product() {
    let db = get_db().await;
    let table = Product::sqlite_table(db);

    let result = table.get_some_value().await.unwrap();
    assert!(result.is_some());

    let (id, record) = result.unwrap();
    assert!(!id.is_empty());
    assert!(record.get("name").is_some());
    assert!(record.get("price").is_some());
}
