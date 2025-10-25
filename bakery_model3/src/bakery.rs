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
        Table::new("bakery", db)
            .with_column("name")
            .with_column("profit_margin")
            .into_entity()
    }
}

pub trait BakeryTable {
    // TODO: Uncomment when relationships are implemented in 0.3
    // fn ref_clients(&self) -> Table<SurrealDB, Client>;
    // fn ref_products(&self) -> Table<SurrealDB, Product>;
}

impl BakeryTable for Table<SurrealDB, Bakery> {
    // TODO: Uncomment when relationships are implemented in 0.3
    // fn ref_clients(&self) -> Table<SurrealDB, Client> {
    //     // Implementation will depend on how relationships are handled in 0.3
    // }
    //
    // fn ref_products(&self) -> Table<SurrealDB, Product> {
    //     // Implementation will depend on how relationships are handled in 0.3
    // }
}
