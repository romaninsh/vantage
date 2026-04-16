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

/// Custom expression condition — columns passed as expression arguments
#[tokio::test]
async fn test_custom_expression_condition() {
    let db = get_db().await;
    let mut table = Product::sqlite_table(db);
    table.add_condition(sqlite_expr!("{} > {}", (table["price"]), 130i64));

    let products = table.list().await.unwrap();
    assert_eq!(products.len(), 4); // all except flux_cupcake (120)
    for (_id, p) in &products {
        assert!(p.price > 130);
    }
}

/// Multiple conditions combine with AND
#[tokio::test]
async fn test_multiple_conditions() {
    let db = get_db().await;
    let mut table = Product::sqlite_table(db);
    // Products with price > 130 AND calories <= 250:
    // delorean_donut (135, 250), time_tart (220, 200), hover_cookies (199, 150)
    table.add_condition(sqlite_expr!("{} > {}", (table["price"]), 130i64));
    table.add_condition(sqlite_expr!("{} <= {}", (table["calories"]), 250i64));

    let products = table.list().await.unwrap();
    assert_eq!(products.len(), 3);
    for (_id, p) in &products {
        assert!(p.price > 130);
        assert!(p.calories <= 250);
    }
}

/// Operation::eq() on table column — the idiomatic way to build conditions
#[tokio::test]
async fn test_operation_eq() {
    let db = get_db().await;
    let mut table = Product::sqlite_table(db);
    table.add_condition(table["is_deleted"].eq(false));

    let products = table.list().await.unwrap();
    assert_eq!(products.len(), 5); // all products have is_deleted = false
}

/// Condition that matches nothing returns empty
#[tokio::test]
async fn test_condition_no_matches() {
    let db = get_db().await;
    let mut table = Product::sqlite_table(db);
    table.add_condition(sqlite_expr!("{} > {}", (table["price"]), 999));

    let products = table.list().await.unwrap();
    assert!(products.is_empty());
}
