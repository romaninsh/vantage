//! Bakery integration tests — query the pre-populated SQLite database
//! using ExprDataSource and SqliteSelect, deserialize into structs.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use vantage_expressions::{ExprDataSource, Expression, Expressive, ExpressiveEnum};
use vantage_sql::sql_expr;
use vantage_sql::sqlite::statements::SqliteSelect;
use vantage_sql::sqlite::{AnySqliteType, SqliteDB};
use vantage_types::{Record, TryFromRecord};

const DB_PATH: &str = "sqlite:../target/bakery.sqlite?mode=ro";

async fn get_db() -> SqliteDB {
    SqliteDB::connect(DB_PATH)
        .await
        .expect("Failed to connect to bakery.sqlite")
}

/// Convert Expression<JsonValue> → Expression<AnySqliteType> by wrapping scalar params.
fn to_typed_expr(expr: Expression<JsonValue>) -> Expression<AnySqliteType> {
    let params = expr
        .parameters
        .into_iter()
        .map(|p| match p {
            ExpressiveEnum::Scalar(v) => {
                ExpressiveEnum::Scalar(AnySqliteType::from_json(&v).unwrap())
            }
            ExpressiveEnum::Nested(nested) => {
                ExpressiveEnum::Nested(to_typed_expr(nested))
            }
            _ => panic!("unexpected deferred in select expression"),
        })
        .collect();
    Expression::new(expr.template, params)
}

/// Execute a SqliteSelect via ExprDataSource, return rows as Vec<Record<JsonValue>>.
async fn exec_select(db: &SqliteDB, select: &SqliteSelect) -> Vec<Record<JsonValue>> {
    let result = db.execute(&to_typed_expr(select.expr())).await.unwrap();
    match result.into_value() {
        JsonValue::Array(arr) => arr.into_iter().map(|v| v.into()).collect(),
        other => panic!("expected array, got: {:?}", other),
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
struct Bakery {
    id: String,
    name: String,
    profit_margin: i64,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
struct Client {
    id: String,
    name: String,
    email: String,
    contact_details: String,
    is_paying_client: bool,
    balance: f64,
    bakery_id: String,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
struct Product {
    id: String,
    name: String,
    calories: i64,
    price: i64,
    bakery_id: String,
    is_deleted: bool,
    inventory_stock: i64,
}

#[tokio::test]
async fn test_read_bakery() {
    let db = get_db().await;
    let rows = exec_select(&db, &SqliteSelect::new().from("bakery")).await;

    assert_eq!(rows.len(), 1);
    let bakery: Bakery = Bakery::from_record(rows[0].clone()).unwrap();
    assert_eq!(bakery.id, "hill_valley");
    assert_eq!(bakery.name, "Hill Valley Bakery");
    assert_eq!(bakery.profit_margin, 15);
}

#[tokio::test]
async fn test_read_clients() {
    let db = get_db().await;
    let rows = exec_select(&db, &SqliteSelect::new().from("client")).await;

    assert_eq!(rows.len(), 3);
    let clients: Vec<Client> = rows
        .into_iter()
        .map(|r| Client::from_record(r).unwrap())
        .collect();

    let marty = clients.iter().find(|c| c.id == "marty").unwrap();
    assert_eq!(marty.name, "Marty McFly");
    assert!(marty.is_paying_client);
    assert!((marty.balance - 150.0).abs() < f64::EPSILON);

    let biff = clients.iter().find(|c| c.id == "biff").unwrap();
    assert!(!biff.is_paying_client);
    assert!((biff.balance - (-50.25)).abs() < f64::EPSILON);
}

#[tokio::test]
async fn test_read_products() {
    let db = get_db().await;
    let rows = exec_select(&db, &SqliteSelect::new().from("product")).await;

    assert_eq!(rows.len(), 5);
    let products: Vec<Product> = rows
        .into_iter()
        .map(|r| Product::from_record(r).unwrap())
        .collect();

    let cupcake = products.iter().find(|p| p.id == "flux_cupcake").unwrap();
    assert_eq!(cupcake.name, "Flux Capacitor Cupcake");
    assert_eq!(cupcake.calories, 300);
    assert_eq!(cupcake.price, 120);
    assert_eq!(cupcake.inventory_stock, 50);
    assert!(!cupcake.is_deleted);
}

#[tokio::test]
async fn test_select_with_where() {
    let db = get_db().await;
    let select = SqliteSelect::new()
        .from("client")
        .with_where(sql_expr!("\"is_paying_client\" = {}", true));
    let rows = exec_select(&db, &select).await;

    assert_eq!(rows.len(), 2);
    let clients: Vec<Client> = rows
        .into_iter()
        .map(|r| Client::from_record(r).unwrap())
        .collect();
    assert!(clients.iter().all(|c| c.is_paying_client));
}

#[tokio::test]
async fn test_select_with_order_and_limit() {
    let db = get_db().await;
    let select = SqliteSelect::new()
        .from("product")
        .with_order_by("price", false)
        .with_limit(2);
    let rows = exec_select(&db, &select).await;

    assert_eq!(rows.len(), 2);
    let products: Vec<Product> = rows
        .into_iter()
        .map(|r| Product::from_record(r).unwrap())
        .collect();
    assert_eq!(products[0].id, "sea_pie");
    assert_eq!(products[1].id, "time_tart");
}

#[tokio::test]
async fn test_select_specific_fields() {
    let db = get_db().await;
    let select = SqliteSelect::new()
        .from("product")
        .field("name")
        .field("price")
        .with_where(sql_expr!("\"id\" = {}", "flux_cupcake"));
    let rows = exec_select(&db, &select).await;

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 2);
    assert_eq!(rows[0]["name"], JsonValue::String("Flux Capacitor Cupcake".into()));
    assert_eq!(rows[0]["price"], JsonValue::Number(120.into()));
}

#[tokio::test]
async fn test_read_orders_with_lines() {
    let db = get_db().await;

    let orders = exec_select(&db, &SqliteSelect::new().from("client_order")).await;
    assert_eq!(orders.len(), 3);

    let doc_orders = exec_select(
        &db,
        &SqliteSelect::new()
            .from("client_order")
            .with_where(sql_expr!("\"client_id\" = {}", "doc")),
    )
    .await;
    assert_eq!(doc_orders.len(), 2);

    let order1_lines = exec_select(
        &db,
        &SqliteSelect::new()
            .from("order_line")
            .with_where(sql_expr!("\"order_id\" = {}", "order1")),
    )
    .await;
    assert_eq!(order1_lines.len(), 3);
}
