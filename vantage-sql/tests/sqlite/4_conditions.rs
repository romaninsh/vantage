//! Test 4: Table conditions — verify that conditions applied via add_condition()
//! flow through to the generated SELECT and filter results correctly.

#[allow(unused_imports)]
use vantage_sql::sqlite::SqliteType;
use vantage_sql::sqlite::operation::SqliteOperation;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_sql::sqlite_expr;
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::ReadableDataSet;

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

/// Multiple conditions combine with AND
#[tokio::test]
async fn test_multiple_conditions() {
    let db = get_db().await;
    let mut table = Product::sqlite_table(db);
    table.add_condition(sqlite_expr!("{} > {}", (table["price"]), 130i64));
    table.add_condition(sqlite_expr!(
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

/// Operation::eq() on table column — the idiomatic way to build conditions
#[tokio::test]
async fn test_operation_eq() {
    let db = get_db().await;
    let mut table = Product::sqlite_table(db);
    table.add_condition(table["price"].eq(220i64));

    let products = table.list().await.unwrap();
    assert_eq!(products.len(), 1);
    assert!(products.contains_key("time_tart"));
}

#[tokio::test]
async fn conditions_describe_themselves_and_name_their_column() {
    use vantage_table::traits::table_source::TableSource;
    let db = get_db().await;
    let a = db
        .eq_value_condition("a", AnySqliteType::from("x"))
        .unwrap();
    let b = db
        .eq_value_condition("b", AnySqliteType::from("y"))
        .unwrap();
    crate::condition_text::assert_condition_text_and_column(
        &db,
        AnySqliteType::from("p1"),
        sqlite_expr!("1 = 1").into(),
        sqlite_expr!("{} AND {}", (a.0), (b.0)).into(),
    );
}

#[tokio::test]
async fn condition_column_names_only_a_column_narrowed_to_values() {
    use vantage_table::column::core::Column;
    let tenant = Column::<AnySqliteType>::new("tenant_id");
    let invoice = Column::<AnySqliteType>::new("invoice_id");
    crate::condition_text::assert_condition_column_names_only_narrowing(
        &get_db().await,
        crate::condition_text::NarrowingShapes {
            negations: vec![
                sqlite_expr!("{} != {}", (tenant), "acme").into(),
                sqlite_expr!("{} <> {}", (tenant), "acme").into(),
                sqlite_expr!("{} NOT IN ({})", (tenant), "acme").into(),
                sqlite_expr!("{} NOT LIKE {}", (tenant), "a%").into(),
            ],
            gt: sqlite_expr!("{} > {}", (invoice), 1i64).into(),
            column_vs_column: sqlite_expr!("{} = {}", (tenant), (tenant)).into(),
            or_operand: sqlite_expr!("{} = {}", (tenant), (sqlite_expr!("7 OR 1 = 1"))).into(),
        },
    );
}

#[tokio::test]
async fn in_values_condition_narrows_to_the_listed_values() {
    crate::condition_text::assert_in_values_narrows_products(&get_db().await, |id| {
        AnySqliteType::from(id)
    })
    .await;
}

#[tokio::test]
async fn condition_column_names_an_in_values_condition() {
    crate::condition_text::assert_in_values_names_its_column(
        &get_db().await,
        AnySqliteType::from("i1"),
    );
}
