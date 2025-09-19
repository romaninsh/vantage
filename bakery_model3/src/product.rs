use serde::{Deserialize, Serialize};

use vantage_expressions::expr;
use vantage_surrealdb::SurrealDB;
use vantage_table::{Column, Entity, Table};

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

impl Entity for Product {}

impl Product {
    pub fn table() -> Table<SurrealDB, Product> {
        Table::new("product", surrealdb())
            .into_entity::<Product>()
            .with(|t| {
                t.add_column(Column::new("name"));
                t.add_column(Column::new("calories"));
                t.add_column(Column::new("price"));
                t.add_column(Column::new("bakery"));
                t.add_column(Column::new("is_deleted"));
                t.add_column(Column::new("inventory"));
            })
    }
}

pub trait ProductTable {
    fn name(&self) -> vantage_expressions::OwnedExpression {
        expr!("name")
    }

    fn calories(&self) -> vantage_expressions::OwnedExpression {
        expr!("calories")
    }

    fn price(&self) -> vantage_expressions::OwnedExpression {
        expr!("price")
    }

    fn bakery(&self) -> vantage_expressions::OwnedExpression {
        expr!("bakery")
    }

    fn is_deleted(&self) -> vantage_expressions::OwnedExpression {
        expr!("is_deleted")
    }

    fn inventory(&self) -> vantage_expressions::OwnedExpression {
        expr!("inventory")
    }

    fn inventory_stock(&self) -> vantage_expressions::OwnedExpression {
        expr!("inventory.stock")
    }

    // TODO: Uncomment when relationships are implemented in 0.3
    // fn ref_bakery(&self) -> Table<SurrealDB, Bakery>;
}

impl ProductTable for Table<SurrealDB, Product> {
    // TODO: Uncomment when relationships are implemented in 0.3
    // fn ref_bakery(&self) -> Table<SurrealDB, Bakery> {
    //     // Implementation will depend on how relationships are handled in 0.3
    // }
}
