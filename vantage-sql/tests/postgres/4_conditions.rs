//! Test 4: Conditions on Table<PostgresDB, Entity>.

#[allow(unused_imports)]
use vantage_sql::postgres::AnyPostgresType;
use vantage_sql::postgres::PostgresDB;
#[allow(unused_imports)]
use vantage_sql::postgres::PostgresType;
use vantage_sql::postgres::operation::PostgresOperation;
use vantage_sql::postgres_expr;
use vantage_table::column::core::Column;
use vantage_table::table::Table;
use vantage_types::entity;

use vantage_dataset::ReadableDataSet;

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
async fn test_custom_expression_condition() {
    let db = get_db().await;
    let mut table = Product::postgres_table(db);
    table.add_condition(postgres_expr!("{} > {}", (table["price"]), 130i64));

    let products = table.list().await.unwrap();
    assert_eq!(products.len(), 4); // 135, 220, 299, 199
}

#[tokio::test]
async fn conditions_describe_themselves_and_name_their_column() {
    use vantage_table::traits::table_source::TableSource;
    let db = get_db().await;
    let a = db
        .eq_value_condition("a", AnyPostgresType::from("x"))
        .unwrap();
    let b = db
        .eq_value_condition("b", AnyPostgresType::from("y"))
        .unwrap();
    crate::condition_text::assert_condition_text_and_column(
        &db,
        AnyPostgresType::from("p1"),
        postgres_expr!("1 = 1").into(),
        postgres_expr!("{} AND {}", (a.0), (b.0)).into(),
    );
}

#[tokio::test]
async fn condition_column_names_only_a_column_narrowed_to_values() {
    let tenant = Column::<AnyPostgresType>::new("tenant_id");
    let invoice = Column::<AnyPostgresType>::new("invoice_id");
    crate::condition_text::assert_condition_column_names_only_narrowing(
        &get_db().await,
        crate::condition_text::NarrowingShapes {
            negations: vec![
                postgres_expr!("{} != {}", (tenant), "acme").into(),
                postgres_expr!("{} <> {}", (tenant), "acme").into(),
                postgres_expr!("{} NOT IN ({})", (tenant), "acme").into(),
                postgres_expr!("{} NOT LIKE {}", (tenant), "a%").into(),
            ],
            gt: postgres_expr!("{} > {}", (invoice), 1i64).into(),
            column_vs_column: postgres_expr!("{} = {}", (tenant), (tenant)).into(),
            or_operand: postgres_expr!("{} = {}", (tenant), (postgres_expr!("7 OR 1 = 1"))).into(),
        },
    );
}

#[tokio::test]
async fn in_values_condition_narrows_to_the_listed_values() {
    crate::condition_text::assert_in_values_narrows_products(&get_db().await, |id| {
        AnyPostgresType::from(id)
    })
    .await;
}

#[tokio::test]
async fn condition_column_names_an_in_values_condition() {
    crate::condition_text::assert_in_values_names_its_column(
        &get_db().await,
        AnyPostgresType::from("i1"),
    );
}

#[tokio::test]
async fn test_multiple_conditions() {
    let db = get_db().await;
    let mut table = Product::postgres_table(db);
    table.add_condition(postgres_expr!("{} > {}", (table["price"]), 130i64));
    table.add_condition(postgres_expr!(
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
    let mut table = Product::postgres_table(db);
    table.add_condition(table["is_deleted"].eq(false));

    let products = table.list().await.unwrap();
    assert_eq!(products.len(), 5); // all products have is_deleted=false
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
