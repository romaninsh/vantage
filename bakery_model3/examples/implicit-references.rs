//! Implicit references — dotted active columns (VAN-102).
//!
//! A dotted name in `with_active_columns` traverses declared `has_one`
//! relations and imports the target's field as a read-only column, aliased
//! under the literal dotted name. `order.client.name` is one hop;
//! `order.client.bakery.name` is two. SQL lowers each into a nested correlated
//! scalar subquery; SurrealDB (see the closing note) lowers into a native
//! idiom path.
//!
//! Run: `cargo run -p bakery_model3 --example implicit-references`

use bakery_model3::*;
use vantage_core::Result;
use vantage_dataset::traits::ReadableValueSet;
use vantage_expressions::{ExprDataSource, Expressive};
use vantage_sql::sqlite_expr;

#[tokio::main]
async fn main() -> Result<()> {
    // A self-contained in-memory database, seeded to mirror the model:
    //   client_order.client_id -> client.id,  client.bakery_id -> bakery.id
    let db = SqliteDB::connect("sqlite::memory:")
        .await
        .map_err(|e| vantage_core::error!("connect failed", details = e.to_string()))?;

    db.execute(&sqlite_expr!(
        "CREATE TABLE bakery (id TEXT PRIMARY KEY, name TEXT NOT NULL, profit_margin INTEGER NOT NULL DEFAULT 0)"
    ))
    .await?;
    db.execute(&sqlite_expr!(
        "CREATE TABLE client (id TEXT PRIMARY KEY, name TEXT NOT NULL, bakery_id TEXT)"
    ))
    .await?;
    db.execute(&sqlite_expr!(
        "CREATE TABLE client_order (id TEXT PRIMARY KEY, client_id TEXT, is_deleted BOOLEAN NOT NULL DEFAULT 0)"
    ))
    .await?;

    db.execute(&sqlite_expr!(
        "INSERT INTO bakery (id, name) VALUES ({}, {})",
        "b1",
        "Sunrise Bakery"
    ))
    .await?;
    db.execute(&sqlite_expr!(
        "INSERT INTO client (id, name, bakery_id) VALUES ({}, {}, {})",
        "c1",
        "Marty",
        "b1"
    ))
    .await?;
    db.execute(&sqlite_expr!(
        "INSERT INTO client_order (id, client_id) VALUES ({}, {}), ({}, {})",
        "o1",
        "c1",
        "o2",
        "c1"
    ))
    .await?;

    // Plain names restrict projection; dotted names traverse has_one relations.
    let orders = Order::sqlite_table(db).with_active_columns(&[
        "id",
        "client_id",
        "client.name",        // one hop:  client_order -> client
        "client.bakery.name", // two hops: client_order -> client -> bakery
    ])?;

    println!("-[ lowered SQL ]------------------------------------");
    println!("{}\n", orders.select().expr().preview());

    println!("-[ rows ]-------------------------------------------");
    for row in orders.list_values().await?.values() {
        // Flat keys, literally the dotted names.
        let get = |k: &str| row.get(k).map(|v| format!("{v}")).unwrap_or_default();
        println!(
            "  order {}: client.name={}, client.bakery.name={}",
            get("id"),
            get("client.name"),
            get("client.bakery.name"),
        );
    }

    // The same call against `Order::surreal_table(db)` lowers `client.name` and
    // `client.bakery.name` into SurrealQL idiom paths (each segment escaped
    // separately) instead of nested subqueries.

    Ok(())
}
