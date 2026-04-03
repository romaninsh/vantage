use vantage_csv::{AnyCsvType, Csv};
use vantage_table::table::Table;
use vantage_types::entity;

use crate::Client;

#[entity(CsvType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Order {
    pub client_id: String,
    pub is_deleted: bool,
    pub created_at: String,
    pub lines: String, // JSON stored as string in CSV
}

impl Order {
    pub fn csv_table(csv: Csv) -> Table<Csv, Order> {
        let csv2 = csv.clone();
        Table::<Csv, Order>::new("order", csv)
            .with_column_of::<String>("client_id")
            .with_column_of::<bool>("is_deleted")
            .with_column_of::<String>("created_at")
            .with_column_of::<String>("lines")
            .with_one("client", "client_id", move || {
                Client::csv_table(csv2.clone())
            })
            .into_entity()
    }
}
