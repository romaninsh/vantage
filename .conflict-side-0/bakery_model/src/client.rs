use crate::{order::Order, postgres, Bakery};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use vantage::prelude::*;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Client {
    pub id: i64,
    pub name: String,
    pub contact_details: String,
    pub bakery_id: i64,
}
impl Entity for Client {}

impl Client {
    pub fn static_table() -> &'static Table<Postgres, Client> {
        static TABLE: OnceLock<Table<Postgres, Client>> = OnceLock::new();

        TABLE.get_or_init(|| {
            Table::new_with_entity("client", postgres())
                .with_id_column("id")
                .with_column("name")
                .with_title_column("email")
                .with_column("contact_details")
                .with_column("is_paying_client")
                .with_column("bakery_id")
                .with_one("bakery", "bakery_id", || Box::new(Bakery::table()))
                .with_many("orders", "client_id", || Box::new(Order::table()))
        })
    }
    pub fn table() -> Table<Postgres, Client> {
        Client::static_table().clone()
    }
}

pub trait ClientTable: SqlTable {
    fn name(&self) -> Arc<PgValueColumn> {
        self.get_column("name").unwrap()
    }
    fn email(&self) -> Arc<PgValueColumn> {
        self.get_column("email").unwrap()
    }
    fn contact_details(&self) -> Arc<PgValueColumn> {
        self.get_column("contact_details").unwrap()
    }
    fn bakery_id(&self) -> Arc<PgValueColumn> {
        self.get_column("bakery_id").unwrap()
    }
    fn is_paying_client(&self) -> Arc<PgValueColumn> {
        self.get_column("is_paying_client").unwrap()
    }

    fn ref_bakery(&self) -> Table<Postgres, Bakery>;
    fn ref_orders(&self) -> Table<Postgres, Order>;
}
impl ClientTable for Table<Postgres, Client> {
    fn ref_bakery(&self) -> Table<Postgres, Bakery> {
        self.get_ref_as("bakery").unwrap()
    }
    fn ref_orders(&self) -> Table<Postgres, Order> {
        self.get_ref_as("orders").unwrap()
    }
}
