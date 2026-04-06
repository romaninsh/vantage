//! Test 4: Table definition and query generation via TableSource.
//!
//! Verifies that `Table<SqliteDB, Entity>` builds correct SELECT queries
//! using column definitions and the Selectable trait.

#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_table::table::Table;
use vantage_types::entity;

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
async fn test_product_select() {
    let db = SqliteDB::connect("sqlite::memory:").await.unwrap();
    let table = Product::sqlite_table(db);
    let select = table.select();
    assert_eq!(
        select.preview(),
        "SELECT \"id\", \"name\", \"calories\", \"price\", \"bakery_id\", \"is_deleted\", \"inventory_stock\" FROM \"product\""
    );
}
