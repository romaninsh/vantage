//! Test 4: Conditions on Table<MysqlDB, Entity>.

#[allow(unused_imports)]
use vantage_sql::mysql::AnyMysqlType;
use vantage_sql::mysql::MysqlDB;
#[allow(unused_imports)]
use vantage_sql::mysql::MysqlType;
use vantage_sql::mysql::operation::MysqlOperation;
use vantage_sql::mysql_expr;
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::ReadableDataSet;

const MYSQL_URL: &str = "mysql://vantage:vantage@localhost:3306/vantage";

async fn get_db() -> MysqlDB {
    MysqlDB::connect(MYSQL_URL).await.unwrap()
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
async fn test_custom_expression_condition() {
    let db = get_db().await;
    let mut table = Product::mysql_table(db);
    table.add_condition(mysql_expr!("{} > {}", (table["price"]), 130i64));

    let products = table.list().await.unwrap();
    assert_eq!(products.len(), 4); // 135, 220, 299, 199
}

#[tokio::test]
async fn test_multiple_conditions() {
    let db = get_db().await;
    let mut table = Product::mysql_table(db);
    table.add_condition(mysql_expr!("{} > {}", (table["price"]), 130i64));
    table.add_condition(mysql_expr!(
        "{} > {}",
        (table["price"]),
        (table["calories"])
    ));

    let products = table.list().await.unwrap();
    // price > 130 AND price > calories:
    // delorean_donut: 135 > 250? no
    // time_tart: 220 > 200? yes
    // sea_pie: 299 > 350? no
    // hover_cookies: 199 > 150? yes
    assert_eq!(products.len(), 2);
}

#[tokio::test]
async fn test_operation_eq() {
    let db = get_db().await;
    let mut table = Product::mysql_table(db);
    table.add_condition(table["is_deleted"].eq(false));

    let products = table.list().await.unwrap();
    assert_eq!(products.len(), 5); // all products have is_deleted=false
}
