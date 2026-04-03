use vantage_csv::{AnyCsvType, Csv};
use vantage_surrealdb::surrealdb::SurrealDB;
use vantage_surrealdb::types::AnySurrealType;
use vantage_table::table::Table;
use vantage_types::entity;

use crate::Client;

#[entity(CsvType, SurrealType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Order {
    pub client_id: Option<String>,
    pub is_deleted: bool,
    pub created_at: Option<String>,
    pub lines: Option<String>, // JSON stored as string in CSV
}

impl Order {
    pub fn csv_table(csv: Csv) -> Table<Csv, Order> {
        let csv2 = csv.clone();
        Table::new("order", csv)
            .with_column_of::<String>("client_id")
            .with_column_of::<bool>("is_deleted")
            .with_column_of::<String>("created_at")
            .with_column_of::<String>("lines")
            .with_one("client", "client_id", move || {
                Client::csv_table(csv2.clone())
            })
    }

    pub fn surreal_table(db: SurrealDB) -> Table<SurrealDB, Order> {
        Table::new("order", db)
            .with_id_column("id")
            .with_column_of::<bool>("is_deleted")
    }
}
