use serde::{Deserialize, Serialize};

use vantage_surrealdb::{SurrealColumn, SurrealDB};
use vantage_table::{ColumnFlag, Table};

use crate::surrealdb;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Client {
    pub name: String,
    pub email: String,
    pub contact_details: String,
    pub is_paying_client: bool,
    pub bakery: String, // Record ID for bakery
    pub metadata: Option<serde_json::Value>,
}

impl Client {
    pub fn table() -> Table<SurrealDB, Client> {
        Table::new("client", surrealdb())
            .with_column(SurrealColumn::new("name").with_flags(&[ColumnFlag::Mandatory]))
            .with_column("email")
            .with_column("contact_details")
            .with_column("is_paying_client")
            .with_column("bakery")
            .with_column("metadata")
            .into_entity()
    }
}

pub trait ClientTable {
    // TODO: Uncomment when relationships are implemented in 0.3
    // fn ref_bakery(&self) -> Table<SurrealDB, Bakery>;
    // fn ref_orders(&self) -> Table<SurrealDB, Order>;
}

impl ClientTable for Table<SurrealDB, Client> {
    // TODO: Uncomment when relationships are implemented in 0.3
    // fn ref_bakery(&self) -> Table<SurrealDB, Bakery> {
    //     // Implementation will depend on how relationships are handled in 0.3
    // }
    //
    // fn ref_orders(&self) -> Table<SurrealDB, Order> {
    //     // Implementation will depend on how relationships are handled in 0.3
    // }
}
