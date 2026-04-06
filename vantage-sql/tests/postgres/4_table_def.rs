//! Test 4: Table definition and query generation via TableSource.

#[allow(unused_imports)]
use vantage_sql::postgres::PostgresType;
use vantage_sql::postgres::{AnyPostgresType, PostgresDB};
use vantage_table::table::Table;
use vantage_types::entity;

const PG_URL: &str = "postgres://vantage:vantage@localhost:5433/vantage";

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
async fn test_product_select() {
    let db = PostgresDB::connect(PG_URL).await.unwrap();
    let table = Product::postgres_table(db);
    let select = table.select();
    assert_eq!(
        select.preview(),
        "SELECT \"id\", \"name\", \"calories\", \"price\", \"bakery_id\", \"is_deleted\", \"inventory_stock\" FROM \"product\""
    );
}
