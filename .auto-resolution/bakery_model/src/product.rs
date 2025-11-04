use crate::postgres;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use vantage::prelude::*;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Product {
    pub name: String,
    pub calories: i64,
    pub bakery_id: i64,
    pub price: i64,
}
impl Entity for Product {}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct ProductInventory {
    id: i64,
    product_id: i64,
    stock: i64,
}
impl Entity for ProductInventory {}

impl Product {
    pub fn static_table() -> &'static Table<Postgres, Product> {
        static TABLE: OnceLock<Table<Postgres, Product>> = OnceLock::new();

        TABLE.get_or_init(|| {
            Table::new_with_entity("product", postgres())
                .with_id_column("id")
                .with_title_column("name")
                .with_column("bakery_id")
                .with_column("calories")
                .with_column("price")

            // .has_one("bakery", "bakery_id", || BakerySet::new())
        })
    }
    pub fn table() -> Table<Postgres, Product> {
        Product::static_table().clone()
    }
}

pub trait ProductTable: AnyTable {
    fn with_inventory(self) -> Table<Postgres, ProductInventory>;

    fn name(&self) -> Arc<PgValueColumn> {
        self.get_column("name").unwrap()
    }
    fn price(&self) -> Arc<PgValueColumn> {
        self.get_column("price").unwrap()
    }
    fn bakery_id(&self) -> Arc<PgValueColumn> {
        self.get_column("bakery_id").unwrap()
    }

    // pub fn stock(&self) -> Arc<Field> {
    //     self.get_join("i").unwrap().get_field("stock").unwrap()
    // }
}

impl ProductTable for Table<Postgres, Product> {
    fn with_inventory(self) -> Table<Postgres, ProductInventory> {
        let t = self.with_join::<ProductInventory, EmptyEntity>(
            Table::new_with_entity("inventory", postgres())
                .with_alias("i")
                .with_id_column("product_id")
                .with_column("stock"),
            "id",
        );
        t
    }
}

pub trait ProductInventoryTable: RelatedTable<Postgres> {
    fn j_stock(&self) -> Table<Postgres, EmptyEntity> {
        let j = self.get_join("i").unwrap();
        j.table().clone()
    }

    fn stock(&self) -> Arc<PgValueColumn> {
        let j = self.j_stock();
        j.get_column("stock").unwrap()
    }
}
impl ProductInventoryTable for Table<Postgres, ProductInventory> {}
