//! Test 4: ReadableDataSet for Table<MysqlDB, Entity>.

#[allow(unused_imports)]
use vantage_sql::mysql::AnyMysqlType;
use vantage_sql::mysql::MysqlDB;
#[allow(unused_imports)]
use vantage_sql::mysql::MysqlType;
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::ReadableDataSet;

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

async fn get_db() -> MysqlDB {
    MysqlDB::connect(MYSQL_URL)
        .await
        .expect("Failed to connect — run scripts/mysql/db/v2.sql first")
}

#[entity(MysqlType)]
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
    fn mysql_table(db: MysqlDB) -> Table<MysqlDB, Product> {
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
    let table = Product::mysql_table(db);

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
    let table = Product::mysql_table(db);

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
    let table = Product::mysql_table(db);

    let result = table.get_some().await.unwrap();
    assert!(result.is_some());

    let (id, product) = result.unwrap();
    assert!(!id.is_empty());
    assert!(!product.name.is_empty());
    assert!(product.price > 0);
}
