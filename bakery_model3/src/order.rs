use serde::{Deserialize, Serialize};
use vantage_dataset::dataset::Result;

use vantage_surrealdb::SurrealDB;
use vantage_table::Table;

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct OrderLine {
    pub product: String, // Record ID for product
    pub quantity: i64,
    pub price: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Order {
    pub bakery: String, // Record ID for bakery
    pub client: String, // Record ID for client
    pub is_deleted: bool,
    pub created_at: Option<String>, // SurrealDB datetime
    pub lines: Vec<OrderLine>,
}

impl Order {
    pub fn table(db: SurrealDB) -> Table<SurrealDB, Order> {
        use crate::{Bakery, Client};
        let db2 = db.clone();
        let db3 = db.clone();
        Table::new("order", db)
            .with_id_column("id")
            .with_column("bakery")
            .with_column("client")
            .with_column("is_deleted")
            .with_column("created_at")
            .with_column("lines")
            .with_one("bakery", "bakery", move || Bakery::table(db2.clone()))
            .with_one("client", "client", move || Client::table(db3.clone()))
            .into_entity()
    }
}

pub trait OrderTable {
    fn ref_bakery(&self) -> Table<SurrealDB, crate::Bakery>;
    fn ref_client(&self) -> Table<SurrealDB, crate::Client>;
}

pub trait OrderTableReports {
    fn generate_report(&self) -> impl std::future::Future<Output = Result<String>> + Send;
}

impl OrderTableReports for Table<SurrealDB, Order> {
    async fn generate_report(&self) -> Result<String> {
        // TODO: Uncomment when get() method is implemented in 0.3
        // let mut report = String::new();
        // for order in self.get().await? {
        //     // Calculate total from embedded lines
        //     let total: i64 = order.lines.iter()
        //         .map(|line| line.quantity * line.price)
        //         .sum();
        //
        //     report.push_str(&format!(
        //         " | Ord {} total: ${:.2}\n",
        //         order.id,
        //         total as f64 / 100.0
        //     ));
        // }
        // if report.is_empty() {
        //     Err(anyhow::anyhow!("No orders found"))
        // } else {
        //     report = format!(" +----------------------------------------------------\n{} +----------------------------------------------------", report);
        //     Ok(report)
        // }

        // Placeholder implementation for now
        Ok("Report generation not yet implemented in 0.3".to_string())
    }
}

impl OrderTable for Table<SurrealDB, Order> {
    fn ref_bakery(&self) -> Table<SurrealDB, crate::Bakery> {
        self.get_ref_as("bakery").unwrap()
    }

    fn ref_client(&self) -> Table<SurrealDB, crate::Client> {
        self.get_ref_as("client").unwrap()
    }
}
