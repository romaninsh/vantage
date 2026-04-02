use vantage_csv::{AnyCsvType, Csv};
use vantage_table::table::Table;
use vantage_types::entity;

use crate::{Bakery, Order};

#[entity(CsvType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Client {
    pub name: String,
    pub email: String,
    pub contact_details: String,
    pub is_paying_client: bool,
    pub bakery_id: String,
}

impl Client {
    pub fn csv_table(csv: Csv) -> Table<Csv, Client> {
        let csv2 = csv.clone();
        let csv3 = csv.clone();
        Table::<Csv, Client>::new("client", csv)
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("contact_details")
            .with_column_of::<bool>("is_paying_client")
            .with_column_of::<String>("bakery_id")
            .with_one("bakery", "bakery_id", move || Bakery::csv_table(csv2.clone()))
            .with_many("orders", "client_id", move || Order::csv_table(csv3.clone()))
            .into_entity()
    }
}
