use serde::{Deserialize, Serialize};

use vantage_expressions::{expr, OwnedExpression};
use vantage_surrealdb::SurrealDB;
use vantage_table::{Column, Entity, Table};

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

impl Entity for Client {}

impl Client {
    pub fn table() -> Table<SurrealDB, Client> {
        Table::new("client", surrealdb())
            .into_entity::<Client>()
            .with(|t| {
                t.add_column(Column::new("name"));
                t.add_column(Column::new("email"));
                t.add_column(Column::new("contact_details"));
                t.add_column(Column::new("is_paying_client"));
                t.add_column(Column::new("bakery"));
                t.add_column(Column::new("metadata"));
            })
    }
}

pub trait ClientTable {
    fn name(&self) -> OwnedExpression {
        expr!("name")
    }

    fn email(&self) -> OwnedExpression {
        expr!("email")
    }

    fn contact_details(&self) -> OwnedExpression {
        expr!("contact_details")
    }

    fn is_paying_client(&self) -> OwnedExpression {
        expr!("is_paying_client")
    }

    fn bakery(&self) -> OwnedExpression {
        expr!("bakery")
    }

    fn metadata(&self) -> OwnedExpression {
        expr!("metadata")
    }

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
