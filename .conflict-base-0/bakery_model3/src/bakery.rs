use serde::{Deserialize, Serialize};

use vantage_surrealdb::SurrealDB;
use vantage_table::Table;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Bakery {
    pub name: String,
    pub profit_margin: i64,
}

impl Bakery {
    pub fn table(db: SurrealDB) -> Table<SurrealDB, Bakery> {
        use crate::{Client, Product};
        let db2 = db.clone();
        let db3 = db.clone();
        Table::new("bakery", db)
            .with_id_column("id")
            .with_column("name")
            .with_column("profit_margin")
            .with_many("clients", "bakery", move || Client::table(db2.clone()))
            .with_many("products", "bakery", move || Product::table(db3.clone()))
            .into_entity()
    }
}

pub trait BakeryTable {
    fn ref_clients(&self) -> Table<SurrealDB, crate::Client>;
    fn ref_products(&self) -> Table<SurrealDB, crate::Product>;
}

impl BakeryTable for Table<SurrealDB, Bakery> {
    fn ref_clients(&self) -> Table<SurrealDB, crate::Client> {
        self.get_ref_as("clients").unwrap()
    }

    fn ref_products(&self) -> Table<SurrealDB, crate::Product> {
        self.get_ref_as("products").unwrap()
    }
}
