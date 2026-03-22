use vantage_csv::{AnyCsvType, Csv};
use vantage_table::table::Table;
use vantage_types::entity;

#[entity(CsvType)]
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Client {
    pub name: String,
    pub email: String,
    pub contact_details: String,
    pub is_paying_client: bool,
}

impl Client {
    pub fn csv_table(csv: Csv) -> Table<Csv, Client> {
        Table::<Csv, Client>::new("client", csv)
            .with_column_of::<String>("name")
            .with_column_of::<String>("email")
            .with_column_of::<String>("contact_details")
            .with_column_of::<bool>("is_paying_client")
            .into_entity()
    }
}

// SurrealDB support commented out during CSV-first development
// impl Client {
//     pub fn table(db: SurrealDB) -> Table<SurrealDB, Client> {
//         Table::new("client", db)
//             .with_column_of::<String>("name")
//             .with_column_of::<String>("email")
//             .with_column_of::<String>("contact_details")
//             .with_column_of::<bool>("is_paying_client")
//             .with_column_of::<Decimal>("balance")
//             .with_column_of::<String>("bakery")
//             .with_column("metadata")
//             .into_entity()
//     }
// }
