//! Example demonstrating how references work in Vantage 0.3
//!
//! This shows the implementation of `ref_bakery()` and `ref_orders()` methods
//! similar to bakery_model 0.2, but using the new AnyTable architecture.

use serde::{Deserialize, Serialize};
use vantage_table::mocks::MockTableSource;
use vantage_table::prelude::*;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Bakery {
    pub name: String,
    pub profit_margin: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Client {
    pub name: String,
    pub email: String,
    pub contact_details: String,
    pub is_paying_client: bool,
    pub bakery_id: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Order {
    pub order_ref: String,
    pub client_id: i64,
    pub total: f64,
}

// Define table factory functions (similar to 0.2's static_table pattern)
impl Bakery {
    pub fn table(ds: MockTableSource) -> Table<MockTableSource, Bakery> {
        Table::new("bakery", ds).into_entity::<Bakery>()
        // In 0.3, we'll add with_many when ready:
        // .with_many("clients", "bakery_id", || Client::table(ds.clone()))
    }
}

impl Client {
    pub fn table(ds: MockTableSource) -> Table<MockTableSource, Client> {
        let ds2 = ds.clone();
        let ds3 = ds.clone();
        Table::new("client", ds)
            .into_entity::<Client>()
            // Define relationships using with_one and with_many
            .with_one("bakery", "bakery_id", move || Bakery::table(ds2.clone()))
            .with_many("orders", "client_id", move || Order::table(ds3.clone()))
    }
}

impl Order {
    pub fn table(ds: MockTableSource) -> Table<MockTableSource, Order> {
        Table::new("order", ds.clone())
            .into_entity::<Order>()
            .with_one("client", "client_id", move || Client::table(ds.clone()))
    }
}

// Define trait for client-specific table operations (similar to 0.2)
pub trait ClientTable {
    fn ref_bakery(&self) -> Table<MockTableSource, Bakery>;
    fn ref_orders(&self) -> Table<MockTableSource, Order>;
}

impl ClientTable for Table<MockTableSource, Client> {
    fn ref_bakery(&self) -> Table<MockTableSource, Bakery> {
        // get_ref_as automatically downcasts from AnyTable to concrete type
        self.get_ref_as("bakery").unwrap()
    }

    fn ref_orders(&self) -> Table<MockTableSource, Order> {
        self.get_ref_as("orders").unwrap()
    }
}

fn main() {
    let ds = MockTableSource::new();

    // Create a client table with relationships
    let clients = Client::table(ds.clone());

    println!("=== Vantage 0.3 References Example ===\n");

    // Using the trait methods (ergonomic API)
    println!("1. Using trait methods:");
    let bakery = clients.ref_bakery();
    println!("   - Got bakery table: {}", bakery.table_name());

    let orders = clients.ref_orders();
    println!("   - Got orders table: {}", orders.table_name());

    // Using get_ref directly (returns AnyTable)
    println!("\n2. Using get_ref (returns AnyTable):");
    let bakery_any = clients.get_ref("bakery").unwrap();
    println!("   - DataSource: {}", bakery_any.datasource_name());
    println!("   - Entity: {}", bakery_any.entity_name());

    // Manual downcasting (if needed)
    println!("\n3. Manual downcasting:");
    let bakery_typed: Table<MockTableSource, Bakery> = bakery_any.downcast().unwrap();
    println!("   - Successfully downcast to Table<MockTableSource, Bakery>");
    println!("   - Table name: {}", bakery_typed.table_name());

    // Chain traversal (orders -> client -> bakery)
    println!("\n4. Chaining references:");
    let order_table = Order::table(ds.clone());
    let _client_from_order = order_table.ref_bakery(); // Would go through client
    println!("   - Traversed: order -> client -> bakery");

    // Type safety demonstration
    println!("\n5. Type safety:");
    println!("   - Type of 'bakery': Table<MockTableSource, Bakery>");
    println!("   - Type of 'orders': Table<MockTableSource, Order>");
    println!("   - All type information preserved!");

    println!("\n=== Key Differences from 0.2 ===");
    println!("✓ No Box<dyn SqlTable> - uses AnyTable instead");
    println!("✓ Works with any TableSource (not just SQL)");
    println!("✓ get_ref_as() provides automatic downcasting");
    println!("✓ Better error messages with type names");
    println!("✓ Same ergonomic API as 0.2");

    println!("\n=== TODO for Full 0.3 Implementation ===");
    println!("⏳ Add condition application (IN subquery, equality)");
    println!("⏳ Implement get_linked_table for JOINs");
    println!("⏳ Add field importing (with_imported_fields)");
    println!("⏳ Support for get_subquery_as");
}

// Trait implementation for Order table
pub trait OrderTable {
    fn ref_bakery(&self) -> Table<MockTableSource, Bakery>;
    fn ref_client(&self) -> Table<MockTableSource, Client>;
}

impl OrderTable for Table<MockTableSource, Order> {
    fn ref_client(&self) -> Table<MockTableSource, Client> {
        self.get_ref_as("client").unwrap()
    }

    fn ref_bakery(&self) -> Table<MockTableSource, Bakery> {
        // Chain: order -> client -> bakery
        self.ref_client().ref_bakery()
    }
}
