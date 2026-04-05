//! Bakery integration tests — query the pre-populated SQLite database
//! using SqliteSelect (Selectable trait) + ExprDataSource.

use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use vantage_expressions::{ExprDataSource, Expressive, Selectable};
use vantage_sql::sqlite::SqliteDB;
use vantage_sql::sqlite::statements::SqliteSelect;
use vantage_sql::sqlite_expr;
use vantage_types::{Record, TryFromRecord};

const DB_PATH: &str = "sqlite:../target/bakery.sqlite?mode=ro";

async fn get_db() -> SqliteDB {
    SqliteDB::connect(DB_PATH)
        .await
        .expect("Failed to connect to bakery.sqlite")
}

fn exec_rows(result: serde_json::Value) -> Vec<Record<JsonValue>> {
    match result {
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
    let select = SqliteSelect::new().with_source("bakery");
    let rows = exec_rows(db.execute(&select.expr()).await.unwrap().into_value());

    assert_eq!(rows.len(), 1);
    let bakery: Bakery = Bakery::from_record(rows[0].clone()).unwrap();
    assert_eq!(bakery.id, "hill_valley");
    assert_eq!(bakery.name, "Hill Valley Bakery");
    assert_eq!(bakery.profit_margin, 15);
}

#[tokio::test]
async fn test_read_clients() {
    let db = get_db().await;
    let select = SqliteSelect::new().with_source("client");
    let rows = exec_rows(db.execute(&select.expr()).await.unwrap().into_value());

    assert_eq!(rows.len(), 3);
    let clients: Vec<Client> = rows
        .into_iter()
        .map(|r| Client::from_record(r).unwrap())
        .collect();

    let marty = clients.iter().find(|c| c.id == "marty").unwrap();
    assert_eq!(marty.name, "Marty McFly");
    assert!(marty.is_paying_client);

    let biff = clients.iter().find(|c| c.id == "biff").unwrap();
    assert!(!biff.is_paying_client);
}

#[tokio::test]
async fn test_read_products() {
    let db = get_db().await;
    let select = SqliteSelect::new().with_source("product");
    let rows = exec_rows(db.execute(&select.expr()).await.unwrap().into_value());

    assert_eq!(rows.len(), 5);
    let products: Vec<Product> = rows
        .into_iter()
        .map(|r| Product::from_record(r).unwrap())
        .collect();

    let cupcake = products.iter().find(|p| p.id == "flux_cupcake").unwrap();
    assert_eq!(cupcake.name, "Flux Capacitor Cupcake");
    assert_eq!(cupcake.price, 120);
}

#[tokio::test]
async fn test_select_with_where() {
    let db = get_db().await;
    let select = SqliteSelect::new()
        .with_source("client")
        .with_condition(sqlite_expr!("\"is_paying_client\" = {}", true));
    let rows = exec_rows(db.execute(&select.expr()).await.unwrap().into_value());

    assert_eq!(rows.len(), 2);
}

#[tokio::test]
async fn test_select_with_order_and_limit() {
    let db = get_db().await;
    let select = SqliteSelect::new()
        .with_source("product")
        .with_order(sqlite_expr!("\"price\""), false)
        .with_limit(Some(2), None);
    let rows = exec_rows(db.execute(&select.expr()).await.unwrap().into_value());

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
        .with_source("product")
        .with_field("name")
        .with_field("price")
        .with_condition(sqlite_expr!("\"id\" = {}", "flux_cupcake"));
    let rows = exec_rows(db.execute(&select.expr()).await.unwrap().into_value());

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].len(), 2);
    assert_eq!(
        rows[0]["name"],
        JsonValue::String("Flux Capacitor Cupcake".into())
    );
    assert_eq!(rows[0]["price"], JsonValue::Number(120.into()));
}

#[tokio::test]
async fn test_read_orders_with_lines() {
    let db = get_db().await;

    let orders = exec_rows(
        db.execute(&SqliteSelect::new().with_source("client_order").expr())
            .await
            .unwrap()
            .into_value(),
    );
    assert_eq!(orders.len(), 3);

    let doc_orders = exec_rows(
        db.execute(
            &SqliteSelect::new()
                .with_source("client_order")
                .with_condition(sqlite_expr!("\"client_id\" = {}", "doc"))
                .expr(),
        )
        .await
        .unwrap()
        .into_value(),
    );
    assert_eq!(doc_orders.len(), 2);

    let order1_lines = exec_rows(
        db.execute(
            &SqliteSelect::new()
                .with_source("order_line")
                .with_condition(sqlite_expr!("\"order_id\" = {}", "order1"))
                .expr(),
        )
        .await
        .unwrap()
        .into_value(),
    );
    assert_eq!(order1_lines.len(), 3);
}
