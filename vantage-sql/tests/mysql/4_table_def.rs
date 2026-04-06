//! Test 4: Table definition and query generation via TableSource.

#[allow(unused_imports)]
use vantage_sql::mysql::AnyMysqlType;
use vantage_sql::mysql::MysqlDB;
#[allow(unused_imports)]
use vantage_sql::mysql::MysqlType;
use vantage_table::table::Table;
use vantage_types::entity;

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

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
async fn test_product_select() {
    let db = MysqlDB::connect(MYSQL_URL).await.unwrap();
    let table = Product::mysql_table(db);
    let select = table.select();
    assert_eq!(
        select.preview(),
        "SELECT `id`, `name`, `calories`, `price`, `bakery_id`, `is_deleted`, `inventory_stock` FROM `product`"
    );
}
