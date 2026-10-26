//! Test 4: Conditions on Table<MysqlDB, Entity>.

#[allow(unused_imports)]
use vantage_sql::mysql::AnyMysqlType;
use vantage_sql::mysql::MysqlDB;
#[allow(unused_imports)]
use vantage_sql::mysql::MysqlType;
use vantage_sql::mysql::operation::MysqlOperation;
use vantage_sql::mysql_expr;
use vantage_table::column::core::Column;
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
async fn conditions_describe_themselves_and_name_their_column() {
    use vantage_table::traits::table_source::TableSource;
    let db = get_db().await;
    let a = db.eq_value_condition("a", AnyMysqlType::from("x")).unwrap();
    let b = db.eq_value_condition("b", AnyMysqlType::from("y")).unwrap();
    crate::condition_text::assert_condition_text_and_column(
        &db,
        AnyMysqlType::from("p1"),
        mysql_expr!("1 = 1").into(),
        mysql_expr!("{} AND {}", (a.0), (b.0)).into(),
    );
}

#[tokio::test]
async fn condition_column_names_only_a_column_narrowed_to_values() {
    let tenant = Column::<AnyMysqlType>::new("tenant_id");
    let invoice = Column::<AnyMysqlType>::new("invoice_id");
    crate::condition_text::assert_condition_column_names_only_narrowing(
        &get_db().await,
        crate::condition_text::NarrowingShapes {
            negations: vec![
                mysql_expr!("{} != {}", (tenant), "acme").into(),
                mysql_expr!("{} <> {}", (tenant), "acme").into(),
                mysql_expr!("{} NOT IN ({})", (tenant), "acme").into(),
                mysql_expr!("{} NOT LIKE {}", (tenant), "a%").into(),
            ],
            gt: mysql_expr!("{} > {}", (invoice), 1i64).into(),
            column_vs_column: mysql_expr!("{} = {}", (tenant), (tenant)).into(),
            or_operand: mysql_expr!("{} = {}", (tenant), (mysql_expr!("7 OR 1 = 1"))).into(),
        },
    );
}

#[tokio::test]
async fn in_values_condition_narrows_to_the_listed_values() {
    crate::condition_text::assert_in_values_narrows_products(&get_db().await, |id| {
        AnyMysqlType::from(id)
    })
    .await;
}

#[tokio::test]
async fn condition_column_names_an_in_values_condition() {
    crate::condition_text::assert_in_values_names_its_column(
        &get_db().await,
        AnyMysqlType::from("i1"),
    );
}

#[tokio::test]
async fn test_multiple_conditions() {
    let db = get_db().await;
    let mut table = Product::mysql_table(db);
    table.add_condition(mysql_expr!("{} > {}", (table["price"]), 130i64));
    table.add_condition(mysql_expr!(
        "{} > {}",
        (table["calories"]),
        (table["price"])
    ));

    let products = table.list().await.unwrap();
    // price > 130 removes flux_cupcake (120, 300 calories);
    // calories > price removes time_tart (200 < 220) and hover_cookies (150 < 199)
    assert_eq!(products.len(), 2);
    assert!(products.contains_key("delorean_donut"));
    assert!(products.contains_key("sea_pie"));
}

#[tokio::test]
async fn test_operation_eq() {
    let db = get_db().await;
    let mut table = Product::mysql_table(db);
    table.add_condition(table["price"].eq(220i64));

    let products = table.list().await.unwrap();
    assert_eq!(products.len(), 1);
    assert!(products.contains_key("time_tart"));
}

#[test]
fn test_typed_column_is_null() {
    let note = Column::<i64>::new("note");
    assert_eq!(note.is_null().into_expr().preview(), "note IS NULL");
}

#[test]
fn test_typed_column_is_not_null() {
    let note = Column::<i64>::new("note");
    assert_eq!(note.is_not_null().into_expr().preview(), "note IS NOT NULL");
}
