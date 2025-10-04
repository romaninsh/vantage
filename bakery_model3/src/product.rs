use serde::{Deserialize, Serialize};

use vantage_surrealdb::SurrealDB;
use vantage_table::Table;

use crate::surrealdb;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Inventory {
    pub stock: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Product {
    pub name: String,
    pub calories: i64,
    pub price: i64,
    pub bakery: String, // Record ID for bakery
    pub is_deleted: bool,
    pub inventory: Inventory,
}

impl Product {
    pub fn table() -> Table<SurrealDB, Product> {
        Table::new("product", surrealdb())
            .with_column("name")
            .with_column("calories")
            .with_column("price")
            .with_column("bakery")
            .with_column("is_deleted")
            .with_column("inventory")
            .into_entity()
    }
}

pub trait ProductTable {
    // TODO: Uncomment when relationships are implemented in 0.3
    // fn ref_bakery(&self) -> Table<SurrealDB, Bakery>;
}

impl ProductTable for Table<SurrealDB, Product> {
    // TODO: Uncomment when relationships are implemented in 0.3
    // fn ref_bakery(&self) -> Table<SurrealDB, Bakery> {
    //     // Implementation will depend on how relationships are handled in 0.3
    // }
}
