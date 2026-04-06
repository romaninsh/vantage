//! Test 4: ReadableDataSet for Table<PostgresDB, Entity>.

#[allow(unused_imports)]
use vantage_sql::postgres::PostgresType;
use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::ReadableDataSet;

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

async fn get_db() -> PostgresDB {
    PostgresDB::connect(PG_URL)
        .await
        .expect("Failed to connect — run scripts/postgres/ingress.sh first")
}

#[entity(PostgresType)]
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
    fn postgres_table(db: PostgresDB) -> Table<PostgresDB, Product> {
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
    let table = Product::postgres_table(db);

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
    let table = Product::postgres_table(db);

    let product = table.get("delorean_donut").await.unwrap();
    assert_eq!(product.name, "DeLorean Doughnut");
    assert_eq!(product.price, 135);
    assert_eq!(product.calories, 250);
}

#[tokio::test]
async fn test_get_some_product() {
    let db = get_db().await;
    let table = Product::postgres_table(db);

    let result = table.get_some().await.unwrap();
    assert!(result.is_some());

    let (id, product) = result.unwrap();
    assert!(!id.is_empty());
    assert!(!product.name.is_empty());
    assert!(product.price > 0);
}
