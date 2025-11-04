use crate::{client::*, postgres, product::*};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use vantage::prelude::*;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Bakery {
    pub name: String,
    pub profit_margin: i64,
}
impl Entity for Bakery {}

impl Bakery {
    pub fn static_table() -> &'static Table<Postgres, Bakery> {
        static TABLE: OnceLock<Table<Postgres, Bakery>> = OnceLock::new();

        TABLE.get_or_init(|| {
            Table::new_with_entity("bakery", postgres())
                .with_id_column("id")
                .with_column("name")
                .with_column("profit_margin")
                .with_many("clients", "bakery_id", || Box::new(Client::table()))
                .with_many("products", "bakery_id", || Box::new(Product::table()))
        })
    }
    pub fn table() -> Table<Postgres, Bakery> {
        Bakery::static_table().clone()
    }
}

pub trait BakeryTable: AnyTable {
    // fields
    fn id(&self) -> Arc<PgValueColumn> {
        self.get_column("id").unwrap()
    }
    fn name(&self) -> Arc<PgValueColumn> {
        self.get_column("name").unwrap()
    }
    fn profit_margin(&self) -> Arc<PgValueColumn> {
        self.get_column("profit_margin").unwrap()
    }

    fn ref_clients(&self) -> Table<Postgres, Client>;
    fn ref_products(&self) -> Table<Postgres, Product>;
}
impl BakeryTable for Table<Postgres, Bakery> {
    fn ref_clients(&self) -> Table<Postgres, Client> {
        self.get_ref_as("clients").unwrap()
    }
    fn ref_products(&self) -> Table<Postgres, Product> {
        self.get_ref_as("products").unwrap()
    }
}
