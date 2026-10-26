//! Test 4: Aggregates — COUNT, SUM, MAX, MIN via Table.

#[allow(unused_imports)]
use vantage_sql::postgres::AnyPostgresType;
use vantage_sql::postgres::PostgresDB;
#[allow(unused_imports)]
use vantage_sql::postgres::PostgresType;
use vantage_table::table::Table;
use vantage_types::entity;

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

async fn get_db() -> PostgresDB {
    PostgresDB::connect(PG_URL).await.unwrap()
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
async fn test_aggregates() {
    let db = get_db().await;
    let table = Product::postgres_table(db);
    assert_eq!(table.get_count().await.unwrap(), 5);
    // 120 + 135 + 220 + 299 + 199 = 973
    assert_eq!(
        table
            .get_sum(&table["price"])
            .await
            .unwrap()
            .try_get::<i64>(),
        Some(973)
    );
    assert_eq!(
        table
            .get_max(&table["price"])
            .await
            .unwrap()
            .try_get::<i64>(),
        Some(299)
    );
    assert_eq!(
        table
            .get_min(&table["price"])
            .await
            .unwrap()
            .try_get::<i64>(),
        Some(120)
    );
}
