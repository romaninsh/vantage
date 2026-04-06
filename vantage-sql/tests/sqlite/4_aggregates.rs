//! Test 4: Aggregate operations — count, sum, max, min via Table methods.

#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_sql::sqlite_expr;
use vantage_table::table::Table;
use vantage_types::entity;

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
async fn test_get_count() {
    let db = get_db().await;
    let table = Product::sqlite_table(db);
    assert_eq!(table.get_count().await.unwrap(), 5);
}

#[tokio::test]
async fn test_get_count_with_condition() {
    let db = get_db().await;
    let mut table = Product::sqlite_table(db);
    table.add_condition(sqlite_expr!("{} > {}", (table["price"]), 150));
    assert_eq!(table.get_count().await.unwrap(), 3); // time_tart (220), sea_pie (299), hover_cookies (199)
}

// prices: 120, 135, 220, 299, 199 → sum = 973
#[tokio::test]
async fn test_get_sum() {
    let db = get_db().await;
    let table = Product::sqlite_table(db);
    let result = table.get_sum(&table["price"]).await.unwrap();
    assert_eq!(result.try_get::<i64>().unwrap(), 973);
}

#[tokio::test]
async fn test_get_max() {
    let db = get_db().await;
    let table = Product::sqlite_table(db);
    let result = table.get_max(&table["price"]).await.unwrap();
    assert_eq!(result.try_get::<i64>().unwrap(), 299);
}

#[tokio::test]
async fn test_get_min() {
    let db = get_db().await;
    let table = Product::sqlite_table(db);
    let result = table.get_min(&table["price"]).await.unwrap();
    assert_eq!(result.try_get::<i64>().unwrap(), 120);
}
