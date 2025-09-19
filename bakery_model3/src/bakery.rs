use serde::{Deserialize, Serialize};

use vantage_expressions::expr;
use vantage_surrealdb::SurrealDB;
use vantage_table::{Column, Entity, Table};

use crate::surrealdb;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Bakery {
    pub name: String,
    pub profit_margin: i64,
}

impl Entity for Bakery {}

impl Bakery {
    pub fn table() -> Table<SurrealDB, Bakery> {
        Table::new("bakery", surrealdb())
            .into_entity::<Bakery>()
            .with(|t| {
                t.add_column(Column::new("name"));
                t.add_column(Column::new("profit_margin"));
            })
    }
}

pub trait BakeryTable {
    fn name(&self) -> vantage_expressions::OwnedExpression {
        expr!("name")
    }

    fn profit_margin(&self) -> vantage_expressions::OwnedExpression {
        expr!("profit_margin")
    }

    // TODO: Uncomment when hasMany is implemented in 0.3
    // fn ref_clients(&self) -> Table<SurrealDB, Client>;
    // fn ref_products(&self) -> Table<SurrealDB, Product>;
}

impl BakeryTable for Table<SurrealDB, Bakery> {
    // TODO: Uncomment when hasMany is implemented in 0.3
    // fn ref_clients(&self) -> Table<SurrealDB, Client> {
    //     // Implementation will depend on how relationships are handled in 0.3
    // }
    //
    // fn ref_products(&self) -> Table<SurrealDB, Product> {
    //     // Implementation will depend on how relationships are handled in 0.3
    // }
}
