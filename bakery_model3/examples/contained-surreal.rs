//! Contained relations against a real SurrealDB.
//!
//! Demonstrates embedded `inventory` (contains-one) and `lines` (contains-many)
//! surfaced as sub-Vistas, where writes patch the parent row in place.
//!
//! Setup:
//!   cd vantage-surrealdb/scripts && ./start.sh && ./ingress.sh
//! Run:
//!   cargo run -p bakery_model3 --example contained-surreal
//!
//! Connects to `bakery/v2` by default (override with `SURREALDB_URL`). This
//! example MUTATES the seeded data (product inventory + order lines).

use bakery_model3::{Order, Product, connect_surrealdb, surrealdb};
use ciborium::Value as CborValue;
use vantage_core::Result;
use vantage_dataset::traits::{InsertableValueSet, ReadableValueSet, WritableValueSet};
use vantage_surrealdb::thing::Thing;
use vantage_surrealdb::types::AnySurrealType;
use vantage_surrealdb::vista::factory::SurrealVistaFactory;
use vantage_types::Record;

fn int(n: i64) -> CborValue {
    CborValue::Integer(n.into())
}

fn field(name: &str, value: CborValue) -> Record<CborValue> {
    let mut r = Record::new();
    r.insert(name.to_string(), value);
    r
}

#[tokio::main]
async fn main() -> Result<()> {
    connect_surrealdb().await?;
    let db = surrealdb();

    // ---- contains-one: product.inventory ---------------------------------
    let products =
        SurrealVistaFactory::new(db.clone()).from_table(Product::surreal_table(db.clone()))?;
    let pid = "product:flux_cupcake".to_string();
    let product = products.get_value(&pid).await?.expect("product exists");
    println!(
        "product.inventory (raw column) = {:?}",
        product.get("inventory")
    );

    let inventory = products.get_ref("inventory", &product)?;
    let stock_before = inventory
        .get_value(&"0".to_string())
        .await?
        .expect("inventory record");
    println!("inventory sub-Vista record = {stock_before:?}");

    inventory
        .patch_value(&"0".to_string(), &field("stock", int(60)))
        .await?;
    let product_after = products.get_value(&pid).await?.unwrap();
    println!(
        "after patching stock -> product.inventory = {:?}\n",
        product_after.get("inventory")
    );

    // ---- contains-many: order.lines --------------------------------------
    let orders =
        SurrealVistaFactory::new(db.clone()).from_table(Order::surreal_table(db.clone()))?;
    let oid = "order:order1".to_string();
    let order = orders.get_value(&oid).await?.expect("order exists");

    let lines = orders.get_ref("lines", &order)?;
    println!("order has {} lines", lines.list_values().await?.len());

    // Patch the first line's quantity (preserves its product record link).
    lines
        .patch_value(&"0".to_string(), &field("quantity", int(99)))
        .await?;

    // Add a brand-new line with a real product record link.
    let product_thing = AnySurrealType::new(Thing::new("product", "time_tart")).into_value();
    let mut new_line = Record::new();
    new_line.insert("product".to_string(), product_thing);
    new_line.insert("quantity".to_string(), int(7));
    new_line.insert("price".to_string(), int(220));
    let added = lines.insert_return_id_value(&new_line).await?;
    println!("added line at index {added}");

    let order_after = orders.get_value(&oid).await?.unwrap();
    let CborValue::Array(stored) = order_after.get("lines").unwrap() else {
        panic!("lines should be an array");
    };
    println!(
        "after edits -> order has {} lines; line[0].quantity = {:?}",
        stored.len(),
        lines
            .get_value(&"0".to_string())
            .await?
            .unwrap()
            .get("quantity")
    );

    Ok(())
}
