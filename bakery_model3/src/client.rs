use serde::{Deserialize, Serialize};

use vantage_surrealdb::SurrealDB;
use vantage_table::{ColumnFlag, Table};

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Client {
    pub name: String,
    pub email: String,
    pub contact_details: String,
    pub is_paying_client: bool,
    pub balance: rust_decimal::Decimal,
    pub bakery: String, // Record ID for bakery
    pub metadata: Option<serde_json::Value>,
}

impl Client {
    pub fn table(db: SurrealDB) -> Table<SurrealDB, Client> {
        use crate::{Bakery, Order};
        use vantage_surrealdb::{prelude::*, SurrealColumn};
        let db2 = db.clone();
        let db3 = db.clone();
        Table::new("client", db)
            .with_id_column("id")
            .with_column(
                SurrealColumn::<String>::new("name")
                    .with_flags(&[ColumnFlag::Mandatory])
                    .into_any(),
            )
            .with_column_of::<String>("email")
            .with_column_of::<String>("contact_details")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<rust_decimal::Decimal>("balance")
            .with_column_of::<String>("bakery")
            .with_column("metadata")
            .with_one("bakery", "bakery", move || Bakery::table(db2.clone()))
            .with_many("orders", "client", move || Order::table(db3.clone()))
            .into_entity()
    }
}

pub trait ClientTable {
    fn ref_bakery(&self) -> Table<SurrealDB, crate::Bakery>;
    fn ref_orders(&self) -> Table<SurrealDB, crate::Order>;
    fn get_paying_balance(&self) -> impl std::future::Future<Output = vantage_core::Result<rust_decimal::Decimal>> + Send;
}

impl ClientTable for Table<SurrealDB, Client> {
    fn ref_bakery(&self) -> Table<SurrealDB, crate::Bakery> {
        self.get_ref_as("bakery").unwrap()
    }

    fn ref_orders(&self) -> Table<SurrealDB, crate::Order> {
        self.get_ref_as("orders").unwrap()
    }

    async fn get_paying_balance(&self) -> vantage_core::Result<rust_decimal::Decimal> {
        use vantage_surrealdb::prelude::*;

        // Create condition for paying clients
        let paying = self.clone().with_condition(self.is_paying_client().eq(true));

        // Create sum expression
        let sum_expr = paying.select().as_sum();

        // Execute and get result
        let sum = sum_expr.execute(self.data_source()).await?;

        Ok(sum)
    }
}
